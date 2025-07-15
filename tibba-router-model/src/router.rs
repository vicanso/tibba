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

use crate::{
    CONFIGURATION_MODEL, CmsModel, FILE_MODEL, HTTP_DETECTOR_MODEL, HTTP_STAT_MODEL, USER_MODEL,
    WEB_PAGE_DETECTOR_MODEL,
};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::MySqlPool;
use std::str::FromStr;
use std::time::Duration;
use tibba_model::{Model, ModelListParams, SchemaView};
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
    State(pool): State<&'static MySqlPool>,
    QueryParams(params): QueryParams<GetSchemaParams>,
) -> CacheJsonResult<SchemaView> {
    let model = CmsModel::from_str(params.name.as_str())
        .map_err(|_| tibba_error::new_error("The model is not supported"))?;
    let view = match model {
        CmsModel::User => USER_MODEL.schema_view(pool).await,
        CmsModel::Configuration => CONFIGURATION_MODEL.schema_view(pool).await,
        CmsModel::File => FILE_MODEL.schema_view(pool).await,
        CmsModel::HttpDetector => HTTP_DETECTOR_MODEL.schema_view(pool).await,
        CmsModel::HttpStat => HTTP_STAT_MODEL.schema_view(pool).await,
        CmsModel::WebPageDetector => WEB_PAGE_DETECTOR_MODEL.schema_view(pool).await,
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
    let model = CmsModel::from_str(params.model.as_str())
        .map_err(|_| tibba_error::new_error("The model is not supported"))?;
    let value = match model {
        CmsModel::User => {
            USER_MODEL
                .list_and_count(pool, params.count, &query_params)
                .await?
        }
        CmsModel::Configuration => {
            CONFIGURATION_MODEL
                .list_and_count(pool, params.count, &query_params)
                .await?
        }
        CmsModel::File => {
            FILE_MODEL
                .list_and_count(pool, params.count, &query_params)
                .await?
        }
        CmsModel::HttpDetector => {
            HTTP_DETECTOR_MODEL
                .list_and_count(pool, params.count, &query_params)
                .await?
        }
        CmsModel::HttpStat => {
            HTTP_STAT_MODEL
                .list_and_count(pool, params.count, &query_params)
                .await?
        }
        CmsModel::WebPageDetector => {
            WEB_PAGE_DETECTOR_MODEL
                .list_and_count(pool, params.count, &query_params)
                .await?
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
    let model = CmsModel::from_str(params.model.as_str())
        .map_err(|_| tibba_error::new_error("The model is not supported"))?;
    let data = match model {
        CmsModel::User => {
            let user = USER_MODEL.get_by_id(pool, params.id).await?;
            json!(user)
        }
        CmsModel::Configuration => {
            let configuration = CONFIGURATION_MODEL.get_by_id(pool, params.id).await?;
            json!(configuration)
        }
        CmsModel::File => {
            let file = FILE_MODEL.get_by_id(pool, params.id).await?;
            json!(file)
        }
        CmsModel::HttpDetector => {
            let detector = HTTP_DETECTOR_MODEL.get_by_id(pool, params.id).await?;
            json!(detector)
        }
        CmsModel::HttpStat => {
            let stat = HTTP_STAT_MODEL.get_by_id(pool, params.id).await?;
            json!(stat)
        }
        CmsModel::WebPageDetector => {
            let detector = WEB_PAGE_DETECTOR_MODEL.get_by_id(pool, params.id).await?;
            json!(detector)
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
    let model = CmsModel::from_str(params.model.as_str())
        .map_err(|_| tibba_error::new_error("The model is not supported"))?;
    match model {
        CmsModel::User => {
            USER_MODEL.delete_by_id(pool, params.id).await?;
        }
        CmsModel::Configuration => {
            CONFIGURATION_MODEL.delete_by_id(pool, params.id).await?;
        }
        CmsModel::File => {
            FILE_MODEL.delete_by_id(pool, params.id).await?;
        }
        CmsModel::HttpDetector => {
            HTTP_DETECTOR_MODEL.delete_by_id(pool, params.id).await?;
        }
        CmsModel::HttpStat => {
            HTTP_STAT_MODEL.delete_by_id(pool, params.id).await?;
        }
        CmsModel::WebPageDetector => {
            WEB_PAGE_DETECTOR_MODEL
                .delete_by_id(pool, params.id)
                .await?;
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
    let model = CmsModel::from_str(params.model.as_str())
        .map_err(|_| tibba_error::new_error("The model is not supported"))?;
    match model {
        CmsModel::User => {
            USER_MODEL
                .update_by_id(pool, params.id, params.data)
                .await?;
        }
        CmsModel::Configuration => {
            CONFIGURATION_MODEL
                .update_by_id(pool, params.id, params.data)
                .await?;
        }
        CmsModel::File => {
            FILE_MODEL
                .update_by_id(pool, params.id, params.data)
                .await?;
        }
        CmsModel::HttpDetector => {
            HTTP_DETECTOR_MODEL
                .update_by_id(pool, params.id, params.data)
                .await?;
        }
        CmsModel::WebPageDetector => {
            WEB_PAGE_DETECTOR_MODEL
                .update_by_id(pool, params.id, params.data)
                .await?;
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
    session: AdminSession,
    JsonParams(params): JsonParams<CreateModelParams>,
) -> JsonResult<Value> {
    let model = CmsModel::from_str(params.model.as_str())
        .map_err(|_| tibba_error::new_error("The model is not supported"))?;
    let mut data = params.data;
    if let Some(obj) = data.as_object_mut() {
        obj.insert("created_by".to_string(), session.get_user_id().into());
    }

    let id = match model {
        CmsModel::Configuration => CONFIGURATION_MODEL.insert(pool, data).await?,
        CmsModel::HttpDetector => HTTP_DETECTOR_MODEL.insert(pool, data).await?,
        CmsModel::WebPageDetector => WEB_PAGE_DETECTOR_MODEL.insert(pool, data).await?,
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
        .route("/schema", get(get_schema).with_state(params.pool))
        .route("/list", get(list_model).with_state(params.pool))
        .route("/detail", get(get_detail).with_state(params.pool))
        .route("/delete", delete(delete_model).with_state(params.pool))
        .route("/update", patch(update_model).with_state(params.pool))
        .route("/create", post(create_model).with_state(params.pool))
}
