use crate::error::{HttpError, HttpResult};
use axum::async_trait;
use axum::body::{Bytes, HttpBody};
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::header::HeaderMap;
use axum::http::request::Parts;
use axum::http::{header, Request};
use axum::response::{IntoResponse, Response};
use axum::BoxError;
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::Serialize;
use validator::Validate;

mod common;
mod inner;
mod user;

// json响应的result
pub(self) type JsonResult<T> = HttpResult<Json<T>>;
// json响应+cache-control
pub(self) struct CacheJson<T>(u32, Json<T>);
// json响应+cache-control的result
pub(self) type CacheJsonResult<T> = HttpResult<CacheJson<T>>;

// tuple转换为cache json
impl<T> From<(u32, T)> for CacheJson<T>
where
    T: Serialize,
{
    fn from(arr: (u32, T)) -> Self {
        CacheJson(arr.0, Json(arr.1))
    }
}

// 实现cache json转换为response
impl<T> IntoResponse for CacheJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        let mut arr = vec!["public".to_string(), format!("max-age={}", self.0)];
        // 如果缓存过长，选择更小的值，避免缓存服务器数据保存过久
        if self.0 > 3600 {
            arr.push("s-maxage=3600".to_string());
        }
        ([(header::CACHE_CONTROL, arr.join(", ").as_str())], self.1).into_response()
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Query<T>(pub T);

#[async_trait]
impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or_default();
        let params = serde_urlencoded::from_str(query)
            .map_err(|err| HttpError::new_with_category(&err.to_string(), "params:from_query"))?;
        Ok(Query(params))
    }
}

struct JsonParams<T>(pub T);

#[async_trait]
impl<T, S, B> FromRequest<S, B> for JsonParams<T>
where
    T: DeserializeOwned + Validate,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<BoxError>,
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request(req: Request<B>, state: &S) -> Result<Self, Self::Rejection> {
        if json_content_type(req.headers()) {
            let bytes = Bytes::from_request(req, state).await.map_err(|err| {
                HttpError::new_with_category(&err.to_string(), "params:read_body")
            })?;
            let deserializer = &mut serde_json::Deserializer::from_slice(&bytes);

            let value: T = match serde_path_to_error::deserialize(deserializer) {
                Ok(value) => value,
                Err(err) => {
                    return Err(HttpError::new_with_category(
                        &err.to_string(),
                        "params:serde_json",
                    ));
                }
            };
            value.validate()?;

            Ok(JsonParams(value))
        } else {
            Err(HttpError::new_with_category(
                "Missing json content type",
                "params:from_json",
            ))
        }
    }
}

fn json_content_type(headers: &HeaderMap) -> bool {
    let content_type = if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        content_type
    } else {
        return false;
    };

    let content_type = if let Ok(content_type) = content_type.to_str() {
        content_type
    } else {
        return false;
    };

    content_type.contains("application/json")
}

pub fn new_router() -> Router {
    Router::new().nest(
        "/api",
        Router::new()
            .merge(common::new_router())
            .merge(user::new_router())
            .merge(inner::new_router()),
    )
}
