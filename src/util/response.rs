use std::str::FromStr;

use axum::http::{header::HeaderName, HeaderMap, HeaderValue};

use crate::error::HTTPResult;

// 插入HTTP响应头
pub fn insert_header(
    headers: &mut HeaderMap<HeaderValue>,
    name: String,
    value: String,
) -> HTTPResult<()> {
    // 如果失败则不设置
    let header_name = HeaderName::from_str(name.as_str())?;
    let header_value = HeaderValue::from_str(value.as_str())?;
    headers.insert(header_name, header_value);
    Ok(())
}

pub fn set_header_if_not_exist(
    headers: &mut HeaderMap<HeaderValue>,
    name: String,
    value: String,
) -> HTTPResult<()> {
    let current = headers.get(name.clone());
    if current.is_some() {
        return Ok(());
    }
    insert_header(headers, name, value)
}

pub fn set_no_cache_if_not_exist(headers: &mut HeaderMap<HeaderValue>) {
    // 因为只会字符导致设置错误
    // 因此此处理不会出错
    let _ = set_header_if_not_exist(headers, "Cache-Control".to_string(), "no-cache".to_string());
}
