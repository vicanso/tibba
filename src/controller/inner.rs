use super::{CacheJsonResult, JsonResult, Query};
use crate::db;
use crate::error::{HttpError, HttpResult};
use crate::middleware::{should_logged_in, Claim};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::middleware::from_fn;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

pub fn new_router() -> Router {
    let r = Router::new()
        .route("/entity-descriptions/:entity", get(get_description))
        .route("/entities/:entity/:id", get(find_by_id))
        .route("/entities/:entity/:id", patch(update_by_id))
        .route("/entities/:entity", post(add))
        .route("/entities/:entity", get(list))
        .layer(from_fn(should_logged_in));

    // TODO 增加鉴权处理
    Router::new().nest("/inners", r)
}

async fn find_by_id(claims: Claim, Path((entity, id)): Path<(String, i64)>) -> JsonResult<Value> {
    let result = db::find_by_id(&entity, &claims.get_account(), id).await?;
    if result.is_none() {
        return Err(HttpError::new("Not found"));
    }
    Ok(result.unwrap().into())
}

#[derive(Debug, Serialize)]
struct AddRecordResp {
    id: i64,
}

async fn add(
    claims: Claim,
    Path(entity): Path<String>,
    Json(value): Json<Value>,
) -> JsonResult<AddRecordResp> {
    let id = db::add(&entity, &claims.get_account(), &value).await?;
    Ok(AddRecordResp { id }.into())
}

#[derive(Debug, Serialize)]
struct ListRecordResp {
    page_count: i64,
    items: Vec<serde_json::Value>,
}
async fn list(
    claims: Claim,
    Path(entity): Path<String>,
    Query(params): Query<db::ListCountParams>,
) -> JsonResult<ListRecordResp> {
    let (page_count, items) = db::list_count(&entity, &claims.get_account(), &params).await?;
    Ok(ListRecordResp { page_count, items }.into())
}

async fn get_description(Path(entity): Path<String>) -> CacheJsonResult<db::EntityDescription> {
    let description = db::description(&entity)?;
    Ok((Duration::from_secs(300), description).into())
}

async fn update_by_id(
    claims: Claim,
    Path((entity, id)): Path<(String, i64)>,
    Json(value): Json<Value>,
) -> HttpResult<StatusCode> {
    db::update_by_id(&entity, &claims.get_account(), id, &value).await?;
    Ok(StatusCode::NO_CONTENT)
}
