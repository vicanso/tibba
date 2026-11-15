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
#[cfg(feature = "oauth")]
use crate::config::must_get_oauth_config;
use crate::config::{must_get_basic_config, must_get_email_config};
use crate::metrics::metrics_handler;
use axum::Router;
use axum::routing::get;
use std::sync::Arc;
use tibba_auth::SessionParams;
use tibba_cache::RedisCache;
use tibba_error::Error;
use tibba_middleware::csrf_token;
use tibba_model::{ConfigurationModel, FileModel, Model, ping_db};
use tibba_router::common::{CommonRouterParams, ReadinessCheck, new_common_router};
#[cfg(feature = "file")]
use tibba_router::file::{FileRouterParams, new_file_router};
use tibba_router::model::{
    ModelAdapter, ModelPermissions, ModelRouterParams, new_model_router, register_model,
    register_model_with,
};
use tibba_router::user::{UserRouterParams, new_user_router};
use tibba_router::user_admin::UserAdminModel;

type Result<T> = std::result::Result<T, Error>;

/// readiness 判定任务队列失控级积压的阈值：pending 超过即认为 worker 完全跟不上，
/// `/readyz` 返回 503 让 K8s 摘流量（减少新任务涌入）。
#[cfg(feature = "job")]
const READINESS_MAX_PENDING: i64 = 5000;

fn register_models(cache: &'static RedisCache, session_params: Arc<SessionParams>) {
    // 敏感模型挂细粒度权限码；Admin 角色仍可通过 authorize 的 admin 逃生舱访问。
    // SuperAdmin 持有 `*`，权限码路径同样放行。未声明权限的模型保持「仅 Admin」。
    // user 模型改身份字段 / 删除后撤销该用户的会话，见 `UserAdminModel`。
    register_model_with(
        "user",
        Arc::new(UserAdminModel::new(cache, session_params)),
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
}

/// 组装完整 HTTP 路由树。
pub fn new_router(ctx: &AppCtx) -> Result<Router> {
    register_models(ctx.cache, Arc::new(crate::config::get_session_params()?));

    let basic_config = must_get_basic_config();
    let pool = ctx.pool;
    let cache = ctx.cache;
    #[cfg(feature = "file")]
    let storage = ctx.storage;
    let state = ctx.state;

    // readiness 深检：依次探测 数据库 → Redis → 可选对象存储 → 可选任务队列，
    // 任一不可达或积压失控即由 /readyz 返回 503，K8s 仅摘流量不重启 Pod。
    let readiness: ReadinessCheck = Arc::new(move || {
        Box::pin(async move {
            ping_db(pool).await?;
            cache.ping().await?;
            #[cfg(feature = "file")]
            storage.check().await?;
            #[cfg(feature = "job")]
            {
                let queue_stats = tibba_job::JobQueue::new(pool).stats().await?;
                if queue_stats.pending > READINESS_MAX_PENDING {
                    return Err(Error::new(format!(
                        "job queue backlog too high: {} pending",
                        queue_stats.pending
                    ))
                    .with_status(503)
                    .with_exception(true));
                }
            }
            Ok(())
        })
    });

    let common_router = new_common_router(CommonRouterParams {
        state,
        readiness: Some(readiness),
    });

    let user_router = new_user_router(UserRouterParams {
        secret: basic_config.secret.clone(),
        pool,
        cache,
        email_config: must_get_email_config(),
        #[cfg(feature = "oauth")]
        oauth_config: must_get_oauth_config(),
        #[cfg(feature = "oauth")]
        oauth_success_redirect: String::new(),
        on_register: None,
    });
    let model_router = new_model_router(ModelRouterParams { pool });

    // API 路由挂在可配置的 prefix 下（如 /api），静态文件始终在根路径。
    #[allow(unused_mut)]
    let mut api_router = Router::new()
        .route("/metrics", get(metrics_handler))
        // 前端启动时调用一次拿 CSRF token，后续状态变更请求把 token 放进 X-CSRF-Token header
        .route("/csrf/token", get(csrf_token))
        .nest("/users", user_router)
        .nest("/models", model_router)
        .merge(common_router);

    #[cfg(feature = "file")]
    {
        api_router = api_router.nest(
            "/files",
            new_file_router(FileRouterParams { storage, pool }),
        );
    }

    #[cfg(feature = "job")]
    {
        api_router = api_router.nest("/jobs", tibba_router::job::new_job_router(pool));
    }

    let app = if let Some(prefix) = &basic_config.prefix {
        Router::new().nest(prefix, api_router)
    } else {
        api_router
    };

    // dev/test 环境挂载 OpenAPI 文档与 Scalar API Reference；生产环境原样返回。
    let app = crate::openapi::mount_swagger(app, basic_config.prefix.as_deref());

    let web_prefix = basic_config.web_prefix.clone().unwrap_or_default();
    Ok(app.fallback(move |uri| {
        let prefix = web_prefix.clone();
        async move { serve_web(&prefix, uri).await }
    }))
}
