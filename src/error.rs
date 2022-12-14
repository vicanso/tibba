use axum::{
    http::{header, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    BoxError, Json,
};
use http::HeaderValue;
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

pub type HTTPResult<T> = Result<T, HTTPError>;

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
    pub fn new_with_status(message: &str, status: u16) -> Self {
        Self {
            message: message.to_string(),
            status,
            ..Default::default()
        }
    }
    pub fn new_with_category_status(message: &str, category: &str, status: u16) -> Self {
        Self {
            message: message.to_string(),
            category: category.to_string(),
            status,
            ..Default::default()
        }
    }
}

impl From<http::header::InvalidHeaderName> for HTTPError {
    fn from(error: http::header::InvalidHeaderName) -> Self {
        HTTPError {
            message: error.to_string(),
            category: "invalidHeaderName".to_string(),
            ..Default::default()
        }
    }
}

impl From<redis::RedisError> for HTTPError {
    fn from(error: redis::RedisError) -> Self {
        HTTPError {
            message: error.to_string(),
            category: "redis".to_string(),
            ..Default::default()
        }
    }
}

impl From<r2d2::Error> for HTTPError {
    fn from(error: r2d2::Error) -> Self {
        HTTPError {
            message: error.to_string(),
            category: "r2d2".to_string(),
            ..Default::default()
        }
    }
}

impl From<http::header::InvalidHeaderValue> for HTTPError {
    fn from(error: http::header::InvalidHeaderValue) -> Self {
        HTTPError {
            message: error.to_string(),
            category: "invalidHeaderValue".to_string(),
            ..Default::default()
        }
    }
}

impl From<serde_json::Error> for HTTPError {
    fn from(error: serde_json::Error) -> Self {
        HTTPError {
            message: error.to_string(),
            category: "serdeJson".to_string(),
            ..Default::default()
        }
    }
}
impl From<std::str::Utf8Error> for HTTPError {
    fn from(error: std::str::Utf8Error) -> Self {
        HTTPError {
            message: error.to_string(),
            category: "utf8".to_string(),
            ..Default::default()
        }
    }
}
impl From<std::io::Error> for HTTPError {
    fn from(error: std::io::Error) -> Self {
        HTTPError {
            message: error.to_string(),
            category: "io".to_string(),
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
    error!("method:{}, uri:{}, error:{}", method, uri, err.to_string());
    if err.is::<tower::timeout::error::Elapsed>() {
        return HTTPError::new_with_category_status("Request took too long", "timeout", 408);
    }
    HTTPError::new(err.to_string().as_str())
}
