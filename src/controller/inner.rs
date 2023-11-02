use super::{JsonResult, Params, Query};
use crate::db::get_database;
use crate::entities::settings;
use crate::error::HttpError;
use crate::middleware::{load_session, Claims};
use crate::util::json_get_string;
use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use sea_orm::query::PaginatorTrait;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait, QueryOrder};
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Serialize)]
struct AddRecordResp {
    id: i64,
}

async fn add(claims: Claims, Params(value): Params<Value>) -> JsonResult<AddRecordResp> {
    let table_name = json_get_string(&value, "table")?.ok_or(HttpError::new("Table is nil"))?;
    let conn = get_database().await;
    let id = match table_name.as_str() {
        TABLE_NAME_SETTINGS => {
            let mut data = settings::ActiveModel::from_value(value)?;
            data.creator = ActiveValue::set(claims.get_account());
            let result = data.insert(conn).await?;
            result.id
        }
        _ => return Err(HttpError::new("Table is invalid")),
    };

    Ok(Json(AddRecordResp { id }))
}

#[derive(Debug, Deserialize)]
struct ListRecordParams {
    table: String,
    orders: Option<String>,
    page: u64,
    page_size: u64,
}

#[derive(Debug, Serialize)]
struct ListRecordResp {
    page_count: u64,
    items: Vec<serde_json::Value>,
}
async fn list(Query(params): Query<ListRecordParams>) -> JsonResult<ListRecordResp> {
    let conn = get_database().await;
    let page_size = params.page_size;
    let page = params.page;
    let orders = params.orders.unwrap_or("-updated_at".to_string());
    let (count, items) = match params.table.as_str() {
        TABLE_NAME_SETTINGS => {
            let count = settings::Entity::find().count(conn).await?;
            let mut sql = settings::Entity::find();
            sql = settings::order_by(sql, &orders)?;
            let items = sql
                .into_json()
                .paginate(conn, page_size)
                .fetch_page(page)
                .await?;
            (count, items)
        }
        _ => return Err(HttpError::new("Table is invalid")),
    };
    let mut page_count = count / page_size;
    if count % page_size != 0 {
        page_count += 1;
    }

    Ok(Json(ListRecordResp { page_count, items }))
}
