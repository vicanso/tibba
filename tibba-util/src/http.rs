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
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::str::FromStr;

type Result<T> = std::result::Result<T, Error>;

/// Insert HTTP headers
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

/// Set HTTP header if it does not exist
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

/// If the cache-control is not set, set it to no-cache
pub fn set_no_cache_if_not_exist(headers: &mut HeaderMap<HeaderValue>) {
    // Because only characters are allowed, setting will not be wrong
    let _ = set_header_if_not_exist(headers, header::CACHE_CONTROL.as_str(), "no-cache");
}

/// Get the value of the HTTP header
pub fn get_header_value(headers: &HeaderMap<HeaderValue>, key: &str) -> String {
    if let Some(value) = headers.get(key) {
        value.to_str().unwrap_or("").to_string()
    } else {
        "".to_string()
    }
}

/// Read the HTTP body
pub async fn read_http_body(body: Body) -> Result<Bytes> {
    let bytes = body
        .collect()
        .await
        .map_err(|e| Error::Axum { source: e })?
        .to_bytes();
    Ok(bytes)
}
