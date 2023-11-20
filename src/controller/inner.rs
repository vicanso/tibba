use super::{JsonResult, Query};
use crate::db::{
    add_setting_json, find_count_setting_json, find_count_user_json, FindRecordParams,
};
use crate::error::HttpError;
use crate::middleware::{load_session, Claim};
use crate::util::json_get_string;
use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::Value;

pub fn new_router() -> Router {
    let r = Router::new()
        .route("/entities", post(add))
        .route("/entities", get(list))
        .layer(from_fn(load_session));

    // TODO 增加鉴权处理
    Router::new().nest("/inners", r)
}

const TABLE_NAME_SETTINGS: &str = "settings";
const TABLE_NAME_USERS: &str = "users";

#[derive(Debug, Serialize)]
struct AddRecordResp {
    id: i64,
}

async fn add(claims: Claim, Json(value): Json<Value>) -> JsonResult<AddRecordResp> {
    let table_name = json_get_string(&value, "table")?.ok_or(HttpError::new("Table is nil"))?;
    let id = match table_name.as_str() {
        TABLE_NAME_SETTINGS => {
            let result = add_setting_json(&claims.get_account(), value).await?;
            result.id
        }
        _ => return Err(HttpError::new("Table is invalid")),
    };

    Ok(Json(AddRecordResp { id }))
}

#[derive(Debug, Serialize)]
struct ListRecordResp {
    page_count: i64,
    items: Vec<serde_json::Value>,
}
async fn list(Query(params): Query<FindRecordParams>) -> JsonResult<ListRecordResp> {
    let (page_count, items) = match params.table.as_str() {
        TABLE_NAME_SETTINGS => find_count_setting_json(params).await?,
        TABLE_NAME_USERS => find_count_user_json(params).await?,
        _ => return Err(HttpError::new("Table is invalid")),
    };

    Ok(Json(ListRecordResp { page_count, items }))
}
