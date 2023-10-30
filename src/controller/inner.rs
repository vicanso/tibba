use super::JsonResult;
use crate::db::get_database;
use crate::entities::settings;
use crate::error::HttpError;
use crate::middleware::{load_session, Claims};
use crate::util::json_get_string;
use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use sea_orm::query::PaginatorTrait;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
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

#[derive(Debug, Serialize)]
pub struct AddRecordResp {
    pub id: i64,
}

async fn add(claims: Claims, Json(value): Json<Value>) -> JsonResult<AddRecordResp> {
    let table_name = json_get_string(&value, "table").ok_or(HttpError::new("Table is nil"))?;
    let conn = get_database().await;
    let id = match table_name.as_str() {
        "settings" => {
            let mut data = settings::ActiveModel::from_value(value);
            data.creator = ActiveValue::set(claims.get_account());
            let result = data.insert(conn).await?;
            result.id
        }
        _ => return Err(HttpError::new("Table is invalid")),
    };

    Ok(Json(AddRecordResp { id }))
}

#[derive(Debug, Serialize)]
pub struct ListRecordResp {
    pub count: i64,
    pub items: Vec<settings::Model>,
}
async fn list() -> JsonResult<ListRecordResp> {
    let conn = get_database().await;
    let items = settings::Entity::find().paginate(conn, 10).fetch().await?;
    Ok(Json(ListRecordResp { count: 1, items }))
}
