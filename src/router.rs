// Copyright 2026 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::admin_web::serve_web;
use crate::app_ctx::AppCtx;
#[cfg(feature = "demo-token")]
use crate::config::must_get_token_config;
use crate::config::{must_get_basic_config, must_get_email_config, must_get_oauth_config};
use crate::metrics::metrics_handler;
use crate::sql::ping_db;
use axum::Router;
use axum::routing::get;
use std::sync::Arc;
use tibba_error::Error;
use tibba_middleware::csrf_token;
use tibba_model::Model;
use tibba_model_builtin::{ConfigurationModel, FileModel, UserModel};
#[cfg(feature = "demo-detector")]
use tibba_model_builtin::{
    DetectorGroupModel, DetectorGroupUserModel, HttpDetectorModel, HttpStatModel,
    WebPageDetectorModel,
};
#[cfg(feature = "demo-token")]
use tibba_model_token::{
    TokenAccountModel, TokenKeyModel, TokenLlmModel, TokenPriceModel, TokenRechargeModel,
    TokenUsageModel,
};
use tibba_router_common::{CommonRouterParams, ReadinessCheck, new_common_router};
use tibba_router_file::{FileRouterParams, new_file_router};
use tibba_router_model::{
    ModelAdapter, ModelPermissions, ModelRouterParams, new_model_router, register_model,
    register_model_with,
};
use tibba_router_user::{UserRouterParams, new_user_router};
use tibba_util::{is_development, is_test};
// error! 仅用于 demo-token 的 on_register 入队失败日志，minimal 构建下无引用
#[cfg(feature = "demo-token")]
use tracing::error;

type Result<T> = std::result::Result<T, Error>;

/// readiness 判定任务队列「失控级」积压的阈值：pending 超过即认为 worker 完全跟不上，
/// `/readyz` 翻红让 K8s 摘流量（减少新任务涌入）。取值很高，避免正常业务积压导致抖动。
const READINESS_MAX_PENDING: i64 = 5000;

fn register_models() {
    // 敏感模型挂细粒度权限码；Admin 角色仍可通过 authorize 的 admin 逃生舱访问。
    // SuperAdmin 持有 `*`，权限码路径同样放行。未声明权限的模型保持「仅 Admin」。
    register_model_with(
        "user",
        Arc::new(ModelAdapter(UserModel::new())),
        ModelPermissions::default()
            .with_read("model:user:read")
            .with_write("model:user:write"),
    );
    register_model_with(
        "configuration",
        Arc::new(ModelAdapter(ConfigurationModel::new())),
        ModelPermissions::default()
            .with_read("model:configuration:read")
            .with_write("model:configuration:write"),
    );
    register_model("file", Arc::new(ModelAdapter(FileModel::new())));
    #[cfg(feature = "demo-detector")]
    {
        register_model(
            "http_detector",
            Arc::new(ModelAdapter(HttpDetectorModel::new())),
        );
        register_model("http_stat", Arc::new(ModelAdapter(HttpStatModel::new())));
        register_model(
            "web_page_detector",
            Arc::new(ModelAdapter(WebPageDetectorModel::new())),
        );
        register_model(
            "detector_group",
            Arc::new(ModelAdapter(DetectorGroupModel::new())),
        );
        register_model(
            "detector_group_user",
            Arc::new(ModelAdapter(DetectorGroupUserModel::new())),
        );
    }
    #[cfg(feature = "demo-token")]
    {
        register_model_with(
            "token_account",
            Arc::new(ModelAdapter(TokenAccountModel::new())),
            ModelPermissions::default()
                .with_read("model:token:read")
                .with_write("model:token:write"),
        );
        register_model_with(
            "token_key",
            Arc::new(ModelAdapter(TokenKeyModel::new())),
            ModelPermissions::default()
                .with_read("model:token:read")
                .with_write("model:token:write"),
        );
        register_model_with(
            "token_recharge",
            Arc::new(ModelAdapter(TokenRechargeModel::new())),
            ModelPermissions::default()
                .with_read("model:token:read")
                .with_write("model:token:write"),
        );
        register_model_with(
            "token_usage",
            Arc::new(ModelAdapter(TokenUsageModel::new())),
            ModelPermissions::default()
                .with_read("model:token:read")
                .with_write("model:token:write"),
        );
        let token_config = must_get_token_config();
        register_model_with(
            "token_price",
            Arc::new(ModelAdapter(
                TokenPriceModel::new().with_model_options(token_config.models.clone()),
            )),
            ModelPermissions::default()
                .with_read("model:token:read")
                .with_write("model:token:write"),
        );
        register_model_with(
            "token_llm",
            Arc::new(ModelAdapter(
                TokenLlmModel::new().with_model_options(token_config.models.clone()),
            )),
            ModelPermissions::default()
                .with_read("model:token:read")
                .with_write("model:token:write"),
        );
    }
}

/// 组装完整 HTTP 路由树。
///
/// `ctx` 为 hook 初始化后的共享依赖（池 / 缓存 / 存储 / AppState），避免在本函数内
/// 再次散落 `get_db_pool()` 等全局读取。配置类仍经 `must_get_*`（启动期一次性读）。
pub fn new_router(ctx: &AppCtx) -> Result<Router> {
    register_models();

    let basic_config = must_get_basic_config();
    let pool = ctx.pool;
    let cache = ctx.cache;
    let storage = ctx.storage;
    let state = ctx.state;
    // readiness 深检：依次探测 数据库 → Redis → 对象存储，任一不可达即由 /readyz
    // 返回 503，K8s 仅摘流量不重启 Pod。顺序由「请求路径强依赖」到「按接口可选」：
    // DB / Redis 几乎所有请求都要用，最该先验；storage 仅部分接口依赖，放最后。
    let readiness: ReadinessCheck = Arc::new(move || {
        Box::pin(async move {
            ping_db().await?;
            cache.ping().await?;
            storage.check().await?;
            // 任务队列可查询（顺带覆盖 jobs 表），且仅在失控级积压时翻红——普通积压不摘流量
            let queue_stats = tibba_job::JobQueue::new(pool).stats().await?;
            if queue_stats.pending > READINESS_MAX_PENDING {
                return Err(Error::new(format!(
                    "job queue backlog too high: {} pending",
                    queue_stats.pending
                ))
                .with_status(503)
                .with_exception(true));
            }
            Ok(())
        })
    });
    let common_router = new_common_router(CommonRouterParams {
        state,
        cache: Some(cache),
        readiness: Some(readiness),
    });
    let mut magic_code = String::new();
    if is_test() || is_development() {
        magic_code = "1234".to_string();
    }
    #[cfg(feature = "demo-token")]
    let on_register = Some(Arc::new(move |user_id| {
        Box::pin(async move {
            // 入队 gift_points 任务：充值由 worker 异步执行，失败自动重试
            let job = tibba_job::Job::new(
                crate::job::JOB_GIFT_POINTS,
                serde_json::json!({ "user_id": user_id }),
            );
            if let Err(e) = tibba_job::JobQueue::new(pool).enqueue(&job).await {
                error!(user_id, error = %e, "入队注册赠积分任务失败");
            }
        }) as tibba_router_user::OnRegisterFuture
    }) as tibba_router_user::OnRegisterFn);
    #[cfg(not(feature = "demo-token"))]
    let on_register = None;

    let user_router = new_user_router(UserRouterParams {
        secret: basic_config.secret.clone(),
        magic_code,
        pool,
        cache,
        email_config: must_get_email_config(),
        oauth_config: must_get_oauth_config(),
        // 空串 → OAuth callback 跳回 "/"，部署侧可在 BasicConfig 加 base_url 后传具体路径
        oauth_success_redirect: String::new(),
        on_register,
    });
    let file_router = new_file_router(FileRouterParams { storage, pool });
    let model_router = new_model_router(ModelRouterParams { pool });

    // 特性开关管理路由（Admin 角色），挂在 /features
    let feature_router = crate::feature::new_feature_router();

    // 异步任务队列 admin 路由（Admin 角色），挂在 /jobs：队列深度概览 + 死信处置
    let job_router = crate::job::new_job_router();

    // API 路由挂在可配置的 prefix 下（如 /api），静态文件始终在根路径
    // /metrics 走 Prometheus text exposition，挂在 API 前缀下，由部署侧通过
    // 内网或鉴权层暴露给 Prometheus / Victoria / Grafana Agent 抓取
    // mut：demo-* feature 开启时会 nest 额外路由
    #[allow(unused_mut)]
    let mut api_router = Router::new()
        .route("/metrics", get(metrics_handler))
        // 前端启动时调用一次拿 CSRF token，后续状态变更请求把 token 放进 X-CSRF-Token header
        .route("/csrf/token", get(csrf_token))
        .nest("/users", user_router)
        .nest("/files", file_router)
        .nest("/models", model_router)
        .nest("/features", feature_router)
        .nest("/jobs", job_router)
        .merge(common_router);

    #[cfg(feature = "demo-docker")]
    {
        use crate::docker::analyze as docker_analyze;
        use axum::routing::post;
        let docker_router = Router::new()
            .route("/analyze", post(docker_analyze))
            .with_state(pool);
        api_router = api_router.nest("/docker", docker_router);
    }

    #[cfg(feature = "demo-tenant")]
    {
        api_router = api_router.nest("/tenant", crate::tenant::new_tenant_router());
    }

    // LLM 流式对话演示 SSE 端点（需登录），挂在 /llm，依赖 token_llms 配置表
    #[cfg(feature = "demo-token")]
    {
        api_router = api_router.nest("/llm", crate::llm::new_llm_router());
    }

    let app = if let Some(prefix) = &basic_config.prefix {
        Router::new().nest(prefix, api_router)
    } else {
        api_router
    };

    // dev/test 环境挂载 Swagger UI（/swagger-ui + /api-docs/openapi.json）；
    // 生产环境原样返回，不暴露 API 表面。servers 用 API 前缀拼出可用的「Try it out」地址。
    let app = crate::openapi::mount_swagger(app, basic_config.prefix.as_deref());

    let web_prefix = basic_config.web_prefix.clone().unwrap_or_default();
    Ok(app.fallback(move |uri| {
        let prefix = web_prefix.clone();
        async move { serve_web(&prefix, uri).await }
    }))
}
