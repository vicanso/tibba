use axum::{
    http::{header, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    BoxError, Json,
};
use http::HeaderValue;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HTTPError {
    // 出错信息
    pub message: String,
    // 出错类型
    pub category: String,
    // 出错码
    pub code: String,
    // HTTP状态码
    pub status: u16,
}

impl Default for HTTPError {
    fn default() -> Self {
        HTTPError {
            message: "".to_string(),
            category: "".to_string(),
            // 默认使用400为状态码
            status: 400,
            code: "".to_string(),
        }
    }
}

impl HTTPError {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            ..Default::default()
        }
    }
    pub fn new_with_category(message: &str, category: &str) -> Self {
        Self {
            message: message.to_string(),
            category: category.to_string(),
            ..Default::default()
        }
    }
}

impl IntoResponse for HTTPError {
    fn into_response(self) -> Response {
        let status = match StatusCode::from_u16(self.status) {
            Ok(status) => status,
            Err(_) => StatusCode::BAD_REQUEST,
        };
        // 对于出错设置为no-cache
        let mut res = Json(self).into_response();
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        (status, res).into_response()
    }
}

pub async fn handle_error(
    // `Method` and `Uri` are extractors so they can be used here
    method: Method,
    uri: Uri,
    // the last argument must be the error itself
    err: BoxError,
) -> HTTPError {
    tracing::error!("method:{}, uri:{}, error:{}", method, uri, err.to_string());
    if err.is::<tower::timeout::error::Elapsed>() {
        let mut he = HTTPError::new_with_category("Request took too long", "timeout");
        he.status = 408;
        return he;
    }
    HTTPError::new(err.to_string().as_str())
}
