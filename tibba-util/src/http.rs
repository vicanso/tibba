// Copyright 2025 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::Error;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderValue, header, header::HeaderName};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use cookie::CookieBuilder;
use http_body_util::BodyExt;
use nanoid::nanoid;
use std::collections::HashMap;
use std::str::FromStr;

// Custom Result type using the crate's Error type
type Result<T> = std::result::Result<T, Error>;

/// Inserts multiple HTTP headers into a HeaderMap
///
/// Safely handles header name and value validation
/// Skips empty names or values
///
/// # Arguments
/// * `headers` - Mutable reference to HeaderMap
/// * `values` - HashMap of header names and values to insert
///
/// # Returns
/// * `Result<()>` - Success or error if header name/value is invalid
pub fn insert_header(
    headers: &mut HeaderMap<HeaderValue>,
    values: HashMap<String, String>,
) -> Result<()> {
    // If it fails, do not set
    for (name, value) in values {
        // If it is empty, do not process (delete using another method)
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let header_name =
            HeaderName::from_str(&name).map_err(|e| Error::InvalidHeaderName { source: e })?;
        let header_value =
            HeaderValue::from_str(&value).map_err(|e| Error::InvalidHeaderValue { source: e })?;
        headers.insert(header_name, header_value);
    }
    Ok(())
}

/// Sets an HTTP header only if it doesn't already exist
///
/// # Arguments
/// * `headers` - Mutable reference to HeaderMap
/// * `name` - Header name
/// * `value` - Header value
///
/// # Returns
/// * `Result<()>` - Success or error if header name/value is invalid
pub fn set_header_if_not_exist(
    headers: &mut HeaderMap<HeaderValue>,
    name: &str,
    value: &str,
) -> Result<()> {
    let current = headers.get(name);
    if current.is_some() {
        return Ok(());
    }
    let values = [(name.to_string(), value.to_string())].into();
    insert_header(headers, values)
}

/// Sets Cache-Control: no-cache header if not already set
///
/// Used to prevent caching of responses when needed
///
/// # Arguments
/// * `headers` - Mutable reference to HeaderMap
pub fn set_no_cache_if_not_exist(headers: &mut HeaderMap<HeaderValue>) {
    // Because only characters are allowed, setting will not be wrong
    let _ = set_header_if_not_exist(headers, header::CACHE_CONTROL.as_str(), "no-cache");
}

/// Retrieves a header value as a String
///
/// Returns empty string if header doesn't exist or value is invalid UTF-8
///
/// # Arguments
/// * `headers` - Reference to HeaderMap
/// * `key` - Header name to retrieve
///
/// # Returns
/// * String containing header value or empty string
pub fn get_header_value(headers: &HeaderMap<HeaderValue>, key: &str) -> String {
    if let Some(value) = headers.get(key) {
        value.to_str().unwrap_or("").to_string()
    } else {
        "".to_string()
    }
}

/// Reads and collects an HTTP body into Bytes
///
/// Useful for accessing the complete body content
///
/// # Arguments
/// * `body` - HTTP Body to read
///
/// # Returns
/// * `Result<Bytes>` - Collected body bytes or error
pub async fn read_http_body(body: Body) -> Result<Bytes> {
    let bytes = body
        .collect()
        .await
        .map_err(|e| Error::Axum { source: e })?
        .to_bytes();
    Ok(bytes)
}

// Name of the device ID cookie
static DEVICE_ID_NAME: &str = "device";

/// Retrieves device ID from cookies
///
/// Returns empty string if device cookie is not present
///
/// # Arguments
/// * `jar` - Reference to CookieJar
///
/// # Returns
/// * String containing device ID or empty string
pub fn get_device_id_from_cookie(jar: &CookieJar) -> String {
    if let Some(value) = jar.get(DEVICE_ID_NAME) {
        return value.value().to_string();
    }
    "".to_string()
}

/// Generates a new device ID cookie
///
/// Creates a cookie with:
/// - 10-character nanoid
/// - 52-week expiration
/// - HTTP-only flag
/// - Root path
///
/// # Returns
/// * CookieBuilder configured with device ID settings
pub fn generate_device_id_cookie() -> CookieBuilder<'static> {
    let expires =
        cookie::time::OffsetDateTime::now_utc().saturating_add(cookie::time::Duration::weeks(52));
    Cookie::build((DEVICE_ID_NAME, nanoid!(10)))
        .http_only(true)
        .expires(expires)
        .path("/")
}
