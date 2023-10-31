use crate::error::HttpError;
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub type Result<T, E = HttpError> = std::result::Result<T, E>;

pub fn json_get(data: &Bytes, key: &str) -> String {
    let message = if let Ok(value) = serde_json::from_slice::<Value>(data) {
        value.get(key).unwrap_or(&Value::Null).to_string()
    } else {
        "".to_string()
    };
    // 处理为""
    if message.to_lowercase() == "null" {
        return "".to_string();
    }
    message
}

pub fn json_get_i64(value: &Value, key: &str) -> Result<Option<i64>> {
    if let Some(value) = value.get(key) {
        if !value.is_i64() {
            return Err(HttpError::new(&format!("{key} is not a number")));
        }
        return Ok(value.as_i64());
    }
    Ok(None)
}

pub fn json_get_string(value: &Value, key: &str) -> Result<Option<String>> {
    if let Some(value) = value.get(key) {
        if !value.is_string() {
            return Err(HttpError::new(&format!("{key} is not a string")));
        }
        if let Some(value) = value.as_str() {
            return Ok(Some(value.to_string()));
        }
    }
    Ok(None)
}

pub fn json_get_date_time(value: &Value, key: &str) -> Result<Option<DateTime<Utc>>> {
    if let Some(value) = json_get_string(value, key)? {
        let value = value
            .parse::<DateTime<Utc>>()
            .map_err(|err| HttpError::new(&err.to_string()))?;
        return Ok(Some(value));
    }
    Ok(None)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Query<T>(pub T);

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
            .map_err(|err| HttpError::new_with_category(&err.to_string(), "query"))?;
        Ok(Query(params))
    }
}
