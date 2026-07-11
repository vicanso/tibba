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

//! 路由组装：脚手架默认挂载 common + user + file + model。
//! 按需 `nest` 业务路由，并从 [`crate::app_ctx::AppCtx`] 取共享依赖。

use crate::admin_web::serve_web;
use crate::app_ctx::AppCtx;
use crate::config::{must_get_basic_config, must_get_email_config, must_get_oauth_config};
use crate::sql::ping_db;
use axum::Router;
use axum::routing::get;
use std::sync::Arc;
use tibba_error::Error;
use tibba_middleware::csrf_token;
use tibba_model::Model;
use tibba_model_builtin::{ConfigurationModel, FileModel, UserModel};
use tibba_router_common::{CommonRouterParams, ReadinessCheck, new_common_router};
use tibba_router_file::{FileRouterParams, new_file_router};
use tibba_router_model::{ModelAdapter, ModelRouterParams, new_model_router, register_model};
use tibba_router_user::{UserRouterParams, new_user_router};
use tibba_util::{is_development, is_test};

type Result<T> = std::result::Result<T, Error>;

fn register_models() {
    register_model("user", Arc::new(ModelAdapter(UserModel::new())));
    register_model(
        "configuration",
        Arc::new(ModelAdapter(ConfigurationModel::new())),
    );
    register_model("file", Arc::new(ModelAdapter(FileModel::new())));
}

/// 组装 HTTP 路由。`ctx` 在 hook 初始化后传入。
pub fn new_router(ctx: &AppCtx) -> Result<Router> {
    register_models();

    let basic_config = must_get_basic_config();
    let pool = ctx.pool;
    let cache = ctx.cache;
    let storage = ctx.storage;
    let state = ctx.state;

    let readiness: ReadinessCheck = Arc::new(move || {
        Box::pin(async move {
            ping_db().await?;
            cache.ping().await?;
            storage.check().await?;
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

    let user_router = new_user_router(UserRouterParams {
        secret: basic_config.secret.clone(),
        magic_code,
        pool,
        cache,
        email_config: must_get_email_config(),
        oauth_config: must_get_oauth_config(),
        oauth_success_redirect: String::new(),
        on_register: None,
    });
    let file_router = new_file_router(FileRouterParams { storage, pool });
    let model_router = new_model_router(ModelRouterParams { pool });

    let api_router = Router::new()
        .route("/csrf/token", get(csrf_token))
        .nest("/users", user_router)
        .nest("/files", file_router)
        .nest("/models", model_router)
        .merge(common_router);

    let app = if let Some(prefix) = &basic_config.prefix {
        Router::new().nest(prefix, api_router)
    } else {
        api_router
    };

    let web_prefix = basic_config.web_prefix.clone().unwrap_or_default();
    Ok(app.fallback(move |uri| {
        let prefix = web_prefix.clone();
        async move { serve_web(&prefix, uri).await }
    }))
}
