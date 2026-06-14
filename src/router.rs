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
use crate::cache::get_redis_cache;
use crate::config::{
    must_get_basic_config, must_get_email_config, must_get_oauth_config, must_get_token_config,
};
use crate::dal::get_opendal_storage;
use crate::docker::analyze as docker_analyze;
use crate::metrics::metrics_handler;
use crate::sql::{get_db_pool, ping_db};
use crate::state::get_app_state;
use axum::Router;
use axum::routing::{get, post};
use std::sync::Arc;
use tibba_error::Error;
use tibba_middleware::csrf_token;
use tibba_model::Model;
use tibba_model_builtin::{
    ConfigurationModel, DetectorGroupModel, DetectorGroupUserModel, FileModel, HttpDetectorModel,
    HttpStatModel, UserModel, WebPageDetectorModel,
};
use tibba_model_token::{
    TokenAccountModel, TokenKeyModel, TokenLlmModel, TokenPriceModel, TokenRechargeModel,
    TokenUsageModel,
};
use tibba_router_common::{CommonRouterParams, ReadinessCheck, new_common_router};
use tibba_router_file::{FileRouterParams, new_file_router};
use tibba_router_model::{ModelAdapter, ModelRouterParams, new_model_router, register_model};
use tibba_router_user::{UserRouterParams, new_user_router};
use tibba_util::{is_development, is_test};
use tracing::error;

type Result<T> = std::result::Result<T, Error>;

fn register_models() {
    register_model("user", Arc::new(ModelAdapter(UserModel::new())));
    register_model(
        "configuration",
        Arc::new(ModelAdapter(ConfigurationModel::new())),
    );
    register_model("file", Arc::new(ModelAdapter(FileModel::new())));
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
    register_model(
        "token_account",
        Arc::new(ModelAdapter(TokenAccountModel::new())),
    );
    register_model("token_key", Arc::new(ModelAdapter(TokenKeyModel::new())));
    register_model(
        "token_recharge",
        Arc::new(ModelAdapter(TokenRechargeModel::new())),
    );
    register_model(
        "token_usage",
        Arc::new(ModelAdapter(TokenUsageModel::new())),
    );
    let token_config = must_get_token_config();
    register_model(
        "token_price",
        Arc::new(ModelAdapter(
            TokenPriceModel::new().with_model_options(token_config.models.clone()),
        )),
    );
    register_model(
        "token_llm",
        Arc::new(ModelAdapter(
            TokenLlmModel::new().with_model_options(token_config.models.clone()),
        )),
    );
}

pub fn new_router() -> Result<Router> {
    register_models();

    let basic_config = must_get_basic_config();
    let cache = get_redis_cache();
    // readiness 深检：依次探测 数据库 → Redis → 对象存储，任一不可达即由 /readyz
    // 返回 503，K8s 仅摘流量不重启 Pod。顺序由「请求路径强依赖」到「按接口可选」：
    // DB / Redis 几乎所有请求都要用，最该先验；storage 仅部分接口依赖，放最后。
    let readiness: ReadinessCheck = Arc::new(|| {
        Box::pin(async {
            ping_db().await?;
            get_redis_cache().ping().await?;
            get_opendal_storage().check().await?;
            Ok(())
        })
    });
    let common_router = new_common_router(CommonRouterParams {
        state: get_app_state(),
        cache: Some(cache),
        readiness: Some(readiness),
    });
    let mut magic_code = String::new();
    if is_test() || is_development() {
        magic_code = "1234".to_string();
    }
    let user_router = new_user_router(UserRouterParams {
        secret: basic_config.secret.clone(),
        magic_code,
        pool: get_db_pool(),
        cache,
        email_config: must_get_email_config(),
        oauth_config: must_get_oauth_config(),
        // 空串 → OAuth callback 跳回 "/"，部署侧可在 BasicConfig 加 base_url 后传具体路径
        oauth_success_redirect: String::new(),
        on_register: Some(Arc::new(|user_id| {
            Box::pin(async move {
                // 入队 gift_points 任务：充值由 worker 异步执行，失败自动重试，
                // 不再 fire-and-forget 丢分（handler 见 crate::job::GiftPointsHandler）
                let job = tibba_job::Job::new(
                    crate::job::JOB_GIFT_POINTS,
                    serde_json::json!({ "user_id": user_id }),
                );
                if let Err(e) = tibba_job::JobQueue::new(get_db_pool()).enqueue(&job).await {
                    error!(user_id, error = %e, "入队注册赠积分任务失败");
                }
            })
        })),
    });
    let file_router = new_file_router(FileRouterParams {
        storage: get_opendal_storage(),
        pool: get_db_pool(),
    });
    let model_router = new_model_router(ModelRouterParams {
        pool: get_db_pool(),
    });

    let docker_router = Router::new()
        .route("/analyze", post(docker_analyze))
        .with_state(get_db_pool());

    // 特性开关管理路由（Admin 角色），挂在 /features
    let feature_router = crate::feature::new_feature_router();

    // API 路由挂在可配置的 prefix 下（如 /api），静态文件始终在根路径
    // /metrics 走 Prometheus text exposition，挂在 API 前缀下，由部署侧通过
    // 内网或鉴权层暴露给 Prometheus / Victoria / Grafana Agent 抓取
    let api_router = Router::new()
        .route("/metrics", get(metrics_handler))
        // 前端启动时调用一次拿 CSRF token，后续状态变更请求把 token 放进 X-CSRF-Token header
        .route("/csrf/token", get(csrf_token))
        .nest("/users", user_router)
        .nest("/files", file_router)
        .nest("/models", model_router)
        .nest("/docker", docker_router)
        .nest("/features", feature_router)
        .merge(common_router);

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
