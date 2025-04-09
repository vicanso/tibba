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
use axum::routing::get;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::MySqlPool;
use tibba_model::{File, ModelListParams, SchemaView, User};
use tibba_session::AdminSession;
use tibba_util::{JsonResult, Query};
use tibba_validator::x_schema_name;
use validator::Validate;

#[derive(Debug, Deserialize, Clone, Validate)]
struct GetSchemaParams {
    #[validate(custom(function = "x_schema_name"))]
    name: String,
}

async fn get_schema(Query(params): Query<GetSchemaParams>) -> JsonResult<SchemaView> {
    let view = match params.name.as_str() {
        "user" => User::schema_view(),
        _ => File::schema_view(),
    };
    Ok(Json(view))
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
    Query(params): Query<ListParams>,
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
        _ => {
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
    };
    Ok(Json(value))
}

pub struct ModelRouterParams {
    pub pool: &'static MySqlPool,
}

pub fn new_model_router(params: ModelRouterParams) -> Router {
    Router::new()
        .route("/schema", get(get_schema))
        .route("/list", get(list_model).with_state(params.pool))
}
