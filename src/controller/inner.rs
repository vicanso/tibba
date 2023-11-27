use super::{JsonResult, Query};
use crate::db;
use crate::error::HttpError;
use crate::middleware::{load_session, Claim};
use crate::util::json_get_string;
use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn new_router() -> Router {
    let r = Router::new()
        .route("/entity-descriptions", get(list_description))
        .route("/entities", post(add))
        .route("/entities", get(list))
        .layer(from_fn(load_session));

    // TODO 增加鉴权处理
    Router::new().nest("/inners", r)
}

#[derive(Debug, Serialize)]
struct AddRecordResp {
    id: i64,
}

async fn add(claims: Claim, Json(value): Json<Value>) -> JsonResult<AddRecordResp> {
    let table_name = json_get_string(&value, "table")?.ok_or(HttpError::new("Table is nil"))?;
    let id = db::add(&table_name, &claims.get_account(), &value).await?;
    Ok(Json(AddRecordResp { id }))
}

#[derive(Debug, Serialize)]
struct ListRecordResp {
    page_count: i64,
    items: Vec<serde_json::Value>,
}
async fn list(Query(params): Query<db::ListCountParams>) -> JsonResult<ListRecordResp> {
    let (page_count, items) = db::list_count(&params).await?;

    Ok(Json(ListRecordResp { page_count, items }))
}

#[derive(Debug, Deserialize)]
pub struct ListDescriptionParams {
    table: String,
}

#[derive(Debug, Serialize)]
struct ListDescriptionResp {
    items: Vec<db::EntityItemDescription>,
}
async fn list_description(
    Query(params): Query<ListDescriptionParams>,
) -> JsonResult<ListDescriptionResp> {
    let descriptions = db::list_descriptions(&params.table)?;
    Ok(Json(ListDescriptionResp {
        items: descriptions,
    }))
}
