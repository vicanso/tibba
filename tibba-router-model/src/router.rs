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
use utoipa::{IntoParams, OpenApi, ToSchema};
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize, Clone, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct GetSchemaParams {
    /// 已注册的模型名（如 `user` / `file`）
    #[validate(custom(function = "x_schema_name"))]
    name: String,
}

#[utoipa::path(
    get,
    path = "/models/schema",
    tag = "model",
    params(GetSchemaParams),
    responses((status = 200, description = "模型的字段视图（schema），用于前端动态渲染表单/列表"))
)]
async fn get_schema(
    State(pool): State<&'static PgPool>,
    QueryParams(params): QueryParams<GetSchemaParams>,
    _session: UserSession,
) -> JsonResult<SchemaView> {
    let model = get_registered_model(&params.name)?;
    Ok(Json(model.schema_view(pool).await))
}

#[derive(Deserialize, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListParams {
    /// 模型名
    model: String,
    /// 页码（从 1 起）
    page: u64,
    /// 每页条数
    limit: u64,
    /// 排序字段，前缀 `-` 表示降序
    order_by: Option<String>,
    /// 关键字模糊搜索
    keyword: Option<String>,
    /// 过滤条件（JSON 编码字符串）
    filters: Option<String>,
    /// 是否同时返回总数
    count: bool,
}

#[utoipa::path(
    get,
    path = "/models/list",
    tag = "model",
    params(ListParams),
    responses((status = 200, description = "分页列表（含可选 count），结构随模型而异"))
)]
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

#[derive(Deserialize, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct GetModelParams {
    /// 模型名
    model: String,
    /// 记录主键
    id: u64,
}

#[utoipa::path(
    get,
    path = "/models/detail",
    tag = "model",
    params(GetModelParams),
    responses(
        (status = 200, description = "单条记录详情，结构随模型而异"),
        (status = 404, description = "记录不存在")
    )
)]
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

#[derive(Deserialize, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct DeleteModelParams {
    /// 模型名
    model: String,
    /// 待删除记录主键
    id: u64,
}

#[utoipa::path(
    delete,
    path = "/models/delete",
    tag = "model",
    params(DeleteModelParams),
    responses((status = 204, description = "删除成功（软删除）"))
)]
async fn delete_model(
    State(pool): State<&'static PgPool>,
    _session: AdminSession,
    QueryParams(params): QueryParams<DeleteModelParams>,
) -> Result<StatusCode> {
    let model = get_registered_model(&params.model)?;
    model.delete_by_id(pool, params.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate, Debug, ToSchema)]
struct UpdateModelParams {
    /// 模型名
    model: String,
    /// 待更新记录主键
    id: u64,
    /// 部分字段更新载荷（按模型 schema 校验）
    #[schema(value_type = Object)]
    data: Value,
}

#[utoipa::path(
    patch,
    path = "/models/update",
    tag = "model",
    request_body = UpdateModelParams,
    responses((status = 204, description = "更新成功"))
)]
async fn update_model(
    State(pool): State<&'static PgPool>,
    _session: AdminSession,
    JsonParams(params): JsonParams<UpdateModelParams>,
) -> Result<StatusCode> {
    let model = get_registered_model(&params.model)?;
    model.update_by_id(pool, params.id, params.data).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate, ToSchema)]
struct CreateModelParams {
    /// 模型名
    model: String,
    /// 新记录字段载荷（按模型 schema 校验）
    #[schema(value_type = Object)]
    data: Value,
}

#[utoipa::path(
    post,
    path = "/models/create",
    tag = "model",
    request_body = CreateModelParams,
    responses((status = 200, description = "创建成功，返回 `{ id }`"))
)]
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

#[derive(Deserialize, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct SearchParams {
    /// 模型名
    model: String,
    /// 关键字（按模型可搜索字段匹配）
    keyword: Option<String>,
}

#[derive(Serialize)]
struct SearchOptionsResp {
    options: Vec<SchemaOption>,
}

#[utoipa::path(
    get,
    path = "/models/search",
    tag = "model",
    params(SearchParams),
    responses((status = 200, description = "下拉选项列表 `{ options: [{ label, value }] }`"))
)]
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

/// 本路由模块的 OpenAPI 文档片段（路径相对 `/models` 已在注解里写全）。
///
/// 列表/详情/schema 等返回随模型而异的动态 JSON，故只描述不绑定具体 body schema；
/// 创建/更新的请求体结构固定，已纳入 components。
#[derive(OpenApi)]
#[openapi(
    paths(
        get_schema,
        list_model,
        search_model,
        get_detail,
        delete_model,
        update_model,
        create_model
    ),
    components(schemas(UpdateModelParams, CreateModelParams)),
    tags((name = "model", description = "通用模型 CRUD（schema 驱动的后台管理）"))
)]
struct ModelApiDoc;

/// 返回 model 路由的 OpenAPI 文档片段，供主 crate 合并进全局文档。
pub fn openapi() -> utoipa::openapi::OpenApi {
    ModelApiDoc::openapi()
}
