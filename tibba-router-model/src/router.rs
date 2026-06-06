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

use crate::{RecordNotFoundSnafu, get_registered_model};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use snafu::OptionExt;
use sqlx::PgPool;
use tibba_error::Error;
use tibba_model::{ModelListParams, SchemaOption, SchemaView};
use tibba_session::{AdminSession, UserSession};
use tibba_util::{JsonParams, JsonResult, QueryParams};
use tibba_validator::x_schema_name;
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize, Clone, Validate)]
struct GetSchemaParams {
    #[validate(custom(function = "x_schema_name"))]
    name: String,
}

async fn get_schema(
    State(pool): State<&'static PgPool>,
    QueryParams(params): QueryParams<GetSchemaParams>,
    _session: UserSession,
) -> JsonResult<SchemaView> {
    let model = get_registered_model(&params.name)?;
    Ok(Json(model.schema_view(pool).await))
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
    State(pool): State<&'static PgPool>,
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
    let model = get_registered_model(&params.model)?;
    Ok(Json(
        model
            .list_and_count(pool, params.count, &query_params)
            .await?,
    ))
}

#[derive(Deserialize, Validate)]
struct GetModelParams {
    model: String,
    id: u64,
}

async fn get_detail(
    State(pool): State<&'static PgPool>,
    QueryParams(params): QueryParams<GetModelParams>,
    _session: AdminSession,
) -> JsonResult<Value> {
    let model = get_registered_model(&params.model)?;
    let data = model
        .get_by_id(pool, params.id)
        .await?
        .context(RecordNotFoundSnafu {
            model: params.model.clone(),
            id: params.id,
        })?;
    Ok(Json(data))
}

#[derive(Deserialize, Validate)]
struct DeleteModelParams {
    model: String,
    id: u64,
}

async fn delete_model(
    State(pool): State<&'static PgPool>,
    _session: AdminSession,
    QueryParams(params): QueryParams<DeleteModelParams>,
) -> Result<StatusCode> {
    let model = get_registered_model(&params.model)?;
    model.delete_by_id(pool, params.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate, Debug)]
struct UpdateModelParams {
    model: String,
    id: u64,
    data: Value,
}

async fn update_model(
    State(pool): State<&'static PgPool>,
    _session: AdminSession,
    JsonParams(params): JsonParams<UpdateModelParams>,
) -> Result<StatusCode> {
    let model = get_registered_model(&params.model)?;
    model.update_by_id(pool, params.id, params.data).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate)]
struct CreateModelParams {
    model: String,
    data: Value,
}

async fn create_model(
    State(pool): State<&'static PgPool>,
    session: AdminSession,
    JsonParams(params): JsonParams<CreateModelParams>,
) -> JsonResult<Value> {
    let model = get_registered_model(&params.model)?;
    let id = model
        .insert(pool, params.data, session.get_user_id())
        .await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize, Validate)]
struct SearchParams {
    model: String,
    keyword: Option<String>,
}

#[derive(Serialize)]
struct SearchOptionsResp {
    options: Vec<SchemaOption>,
}

async fn search_model(
    State(pool): State<&'static PgPool>,
    QueryParams(params): QueryParams<SearchParams>,
    _session: UserSession,
) -> JsonResult<SearchOptionsResp> {
    let model = get_registered_model(&params.model)?;
    let options = model.search_options(pool, params.keyword).await?;
    Ok(Json(SearchOptionsResp { options }))
}

pub struct ModelRouterParams {
    pub pool: &'static PgPool,
}

pub fn new_model_router(params: ModelRouterParams) -> Router {
    Router::new()
        .route("/schema", get(get_schema).with_state(params.pool))
        .route("/list", get(list_model).with_state(params.pool))
        .route("/search", get(search_model).with_state(params.pool))
        .route("/detail", get(get_detail).with_state(params.pool))
        .route("/delete", delete(delete_model).with_state(params.pool))
        .route("/update", patch(update_model).with_state(params.pool))
        .route("/create", post(create_model).with_state(params.pool))
}
