// Copyright 2025 Tree xie.
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

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::MySqlPool;
use std::time::Duration;
use tibba_model::{Configuration, File, HttpDetector, ModelListParams, SchemaView, User};
use tibba_session::AdminSession;
use tibba_util::{CacheJsonResult, JsonParams, JsonResult, QueryParams};
use tibba_validator::x_schema_name;
use validator::Validate;

#[derive(Debug, Deserialize, Clone, Validate)]
struct GetSchemaParams {
    #[validate(custom(function = "x_schema_name"))]
    name: String,
}

async fn get_schema(
    QueryParams(params): QueryParams<GetSchemaParams>,
) -> CacheJsonResult<SchemaView> {
    let view = match params.name.as_str() {
        "user" => User::schema_view(),
        "configuration" => Configuration::schema_view(),
        "file" => File::schema_view(),
        "http_detector" => HttpDetector::schema_view(),
        _ => return Err(tibba_error::new_error("The schema is not found").into()),
    };
    Ok((Duration::from_secs(5 * 60), view).into())
}

#[derive(Deserialize, Validate)]
struct ListParams {
    model: String,
    page: u64,
    limit: u64,
    order_by: Option<String>,
    keyword: Option<String>,
    filters: Option<String>,
    count: bool,
}

async fn list_model(
    State(pool): State<&'static MySqlPool>,
    QueryParams(params): QueryParams<ListParams>,
    _session: AdminSession,
) -> JsonResult<Value> {
    let query_params = ModelListParams {
        page: params.page,
        limit: params.limit,
        order_by: params.order_by,
        keyword: params.keyword,
        filters: params.filters,
    };
    let value = match params.model.as_str() {
        "user" => {
            let count = if params.count {
                User::count(pool, &query_params).await?
            } else {
                -1
            };
            let users = User::list(pool, &query_params).await?;
            json!({
            "count": count,
                    "items": users,
                })
        }
        "configuration" => {
            let count = if params.count {
                Configuration::count(pool, &query_params).await?
            } else {
                -1
            };
            let configurations = Configuration::list(pool, &query_params).await?;
            json!({
            "count": count,
                    "items": configurations,
                })
        }
        "file" => {
            let count = if params.count {
                File::count(pool, &query_params).await?
            } else {
                -1
            };
            let files = File::list(pool, &query_params).await?;
            json!({
            "count": count,
                    "items": files,
                })
        }
        "http_detector" => {
            let count = if params.count {
                HttpDetector::count(pool, &query_params).await?
            } else {
                -1
            };
            let detectors = HttpDetector::list(pool, &query_params).await?;
            json!({
            "count": count,
                    "items": detectors,
                })
        }
        _ => {
            return Err(tibba_error::new_error("The model is not supported").into());
        }
    };
    Ok(Json(value))
}

#[derive(Deserialize, Validate)]
struct GetModelParams {
    model: String,
    id: u64,
}

async fn get_detail(
    State(pool): State<&'static MySqlPool>,
    QueryParams(params): QueryParams<GetModelParams>,
    _session: AdminSession,
) -> JsonResult<Value> {
    let data = match params.model.as_str() {
        "user" => {
            let user = User::get_by_id(pool, params.id).await?;
            json!(user)
        }
        "configuration" => {
            let configuration = Configuration::get_by_id(pool, params.id).await?;
            json!(configuration)
        }
        "file" => {
            let file = File::get_by_id(pool, params.id).await?;
            json!(file)
        }
        "http_detector" => {
            let detector = HttpDetector::get_by_id(pool, params.id).await?;
            json!(detector)
        }
        _ => {
            return Err(tibba_error::new_error("The model is not supported").into());
        }
    };
    if data.is_null() {
        return Err(tibba_error::new_error("The record is not found").into());
    }
    Ok(Json(data))
}

#[derive(Deserialize, Validate)]
struct DeleteModelParams {
    model: String,
    id: u64,
}

async fn delete_model(
    State(pool): State<&'static MySqlPool>,
    _session: AdminSession,
    QueryParams(params): QueryParams<DeleteModelParams>,
) -> Result<StatusCode, tibba_error::Error> {
    match params.model.as_str() {
        "user" => {
            User::delete_by_id(pool, params.id).await?;
        }
        "configuration" => {
            Configuration::delete_by_id(pool, params.id).await?;
        }
        "file" => {
            File::delete_by_id(pool, params.id).await?;
        }
        "http_detector" => {
            HttpDetector::delete_by_id(pool, params.id).await?;
        }
        _ => {
            return Err(tibba_error::new_error("The model is not supported").into());
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate, Debug)]
struct UpdateModelParams {
    model: String,
    id: u64,
    data: Value,
}

async fn update_model(
    State(pool): State<&'static MySqlPool>,
    _session: AdminSession,
    JsonParams(params): JsonParams<UpdateModelParams>,
) -> Result<StatusCode, tibba_error::Error> {
    match params.model.as_str() {
        "user" => {
            User::update_by_id(pool, params.id, params.data.into()).await?;
        }
        "configuration" => {
            Configuration::update_by_id(pool, params.id, params.data.into()).await?;
        }
        "file" => {
            File::update_by_id(pool, params.id, params.data.into()).await?;
        }
        _ => {
            return Err(tibba_error::new_error("The model is not supported").into());
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate)]
struct CreateModelParams {
    model: String,
    data: Value,
}

async fn create_model(
    State(pool): State<&'static MySqlPool>,
    _session: AdminSession,
    JsonParams(params): JsonParams<CreateModelParams>,
) -> JsonResult<Value> {
    let id = match params.model.as_str() {
        "configuration" => Configuration::insert(pool, params.data.into()).await?,
        _ => {
            return Err(tibba_error::new_error("The model is not supported").into());
        }
    };
    Ok(Json(json!({
        "id": id,
    })))
}

pub struct ModelRouterParams {
    pub pool: &'static MySqlPool,
}

pub fn new_model_router(params: ModelRouterParams) -> Router {
    Router::new()
        .route("/schema", get(get_schema))
        .route("/list", get(list_model).with_state(params.pool))
        .route("/detail", get(get_detail).with_state(params.pool))
        .route("/delete", delete(delete_model).with_state(params.pool))
        .route("/update", patch(update_model).with_state(params.pool))
        .route("/create", post(create_model).with_state(params.pool))
}
