use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Serialize;

use crate::error::HTTPResult;

mod common;
mod user;

// json响应的result
pub type JSONResult<T> = HTTPResult<Json<T>>;
// json响应+cache-control
pub struct CacheJSON<T>(u32, Json<T>);
// json响应+cache-control的result
pub type CacheJSONResult<T> = HTTPResult<CacheJSON<T>>;

// tuple转换为cache json
impl<T> From<(u32, T)> for CacheJSON<T>
where
    T: Serialize,
{
    fn from(arr: (u32, T)) -> Self {
        CacheJSON(arr.0, Json(arr.1))
    }
}

// 实现cache json转换为response
impl<T> IntoResponse for CacheJSON<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        let mut arr = vec![
            "public".to_string(),
            format!("max-age={}", self.0).to_string(),
        ];
        // 如果缓存过长，请选择更小的值，避免缓存服务器数据保存过久
        if self.0 > 3600 {
            arr.push("s-maxage=3600".to_string());
        }
        ([(header::CACHE_CONTROL, arr.join(", ").as_str())], self.1).into_response()
    }
}

pub fn new_router() -> Router {
    let r = Router::new();
    r.merge(common::new_router()).merge(user::new_router())
}
