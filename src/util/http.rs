use crate::error::{HttpError, HttpResult};
use axum::body::{Bytes, Body};
use axum::extract::Request;
use axum::http::{header, header::HeaderName, HeaderMap, HeaderValue};
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::str::FromStr;
/// 插入HTTP头
pub fn insert_header(
    headers: &mut HeaderMap<HeaderValue>,
    values: HashMap<String, String>,
) -> HttpResult<()> {
    // 如果失败则不设置
    for (name, value) in values {
        // 为空则不处理（删除使用另外的方式）
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let header_name = HeaderName::from_str(&name)
            .map_err(|err| HttpError::new_with_category(&err.to_string(), "invalid_header_name"))?;
        let header_value = HeaderValue::from_str(&value).map_err(|err| {
            HttpError::new_with_category(&err.to_string(), "invalid_header_value")
        })?;
        headers.insert(header_name, header_value);
    }
    Ok(())
}

/// HTTP头不存在时才设置
pub fn set_header_if_not_exist(
    headers: &mut HeaderMap<HeaderValue>,
    name: &str,
    value: &str,
) -> HttpResult<()> {
    let current = headers.get(name);
    if current.is_some() {
        return Ok(());
    }
    let values = [(name.to_string(), value.to_string())].into();
    insert_header(headers, values)
}

/// 如果未设置cache-control，则设置为no-cache
pub fn set_no_cache_if_not_exist(headers: &mut HeaderMap<HeaderValue>) {
    // 因为只会字符导致设置错误
    // 因此此处理不会出错
    let _ = set_header_if_not_exist(headers, header::CACHE_CONTROL.as_str(), "no-cache");
}

/// 获取http头的值
pub fn get_header_value(headers: &HeaderMap<HeaderValue>, key: &str) -> String {
    if let Some(value) = headers.get(key) {
        value.to_str().unwrap_or("").to_string()
    } else {
        "".to_string()
    }
}

/// 读取http body
pub async fn read_http_body(body: Body) -> HttpResult<Bytes> {
    let bytes = body
        .collect()
        .await
        .map_err(|err| HttpError::new_with_category(&err.to_string(), "body_to_bytes"))?
        .to_bytes();
    Ok(bytes)
}
