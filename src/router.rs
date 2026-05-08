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

use crate::cache::get_redis_cache;
use crate::config::must_get_basic_config;
use crate::dal::get_opendal_storage;
use crate::docker::analyze as docker_analyze;
use crate::sql::get_db_pool;
use crate::state::get_app_state;
use crate::admin_web::serve_web;
use axum::Router;
use axum::routing::post;
use std::sync::Arc;
use tibba_error::Error;
use tibba_model::Model;
use tibba_model_builtin::{
    ConfigurationModel, DetectorGroupModel, DetectorGroupUserModel, FileModel, HttpDetectorModel,
    HttpStatModel, UserModel, WebPageDetectorModel,
};
use tibba_model_token::{
    RECHARGE_SOURCE_GIFT, TokenAccountModel, TokenKeyModel, TokenLlmModel, TokenPriceModel,
    TokenRechargeInsertParams, TokenRechargeModel, TokenService, TokenUsageModel,
};
use tibba_router_common::{CommonRouterParams, new_common_router};
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
    register_model(
        "token_price",
        Arc::new(ModelAdapter(TokenPriceModel::new())),
    );
    register_model("token_llm", Arc::new(ModelAdapter(TokenLlmModel::new())));
}

pub fn new_router() -> Result<Router> {
    register_models();

    let basic_config = must_get_basic_config();
    let cache = get_redis_cache();
    let common_router = new_common_router(CommonRouterParams {
        state: get_app_state(),
        cache: Some(cache),
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
        on_register: Some(Arc::new(|user_id| {
            Box::pin(async move {
                if let Err(e) = TokenService::recharge(
                    get_db_pool(),
                    TokenRechargeInsertParams {
                        user_id,
                        amount: 1_000_000,
                        source: RECHARGE_SOURCE_GIFT,
                        remark: Some("注册赠送".to_string()),
                        ..Default::default()
                    },
                )
                .await
                {
                    error!(user_id, error = %e, "注册赠送积分失败");
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

    // API 路由挂在可配置的 prefix 下（如 /api），静态文件始终在根路径
    let api_router = Router::new()
        .nest("/users", user_router)
        .nest("/files", file_router)
        .nest("/models", model_router)
        .nest("/docker", docker_router)
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
