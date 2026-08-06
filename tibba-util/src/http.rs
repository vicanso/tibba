// Copyright 2026 Tree xie.
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

use super::{AxumSnafu, Error, InvalidHeaderNameSnafu, InvalidHeaderValueSnafu, is_development};
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderValue, header, header::HeaderName};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use cookie::{CookieBuilder, SameSite};
use http_body_util::BodyExt;
use nanoid::nanoid;
use snafu::ResultExt;
use std::time::Duration;

// Custom Result type using the crate's Error type
type Result<T> = std::result::Result<T, Error>;

/// 批量写入 HTTP 头，名称或值为空的条目直接跳过。
///
/// # 失败语义：逐条原子，整体**非**原子
/// 单个条目的名称与值都通过校验后才会写入，不存在「只写了名字没写值」的中间态。
/// 但遇到非法条目会立即返回 `Err`，**此前已写入的条目保留**，`headers` 处于
/// 部分修改状态。
///
/// 之所以不做整体原子（先全部校验再统一写入）：那需要把条目先收集到一个临时
/// Vec，而最主要的调用方 [`set_header_if_not_exist`] 每次只传一个条目、且在
/// 每个响应上都会被调用，为一个当前无人依赖的保证在热路径上加一次堆分配并不划算。
///
/// 调用方若确实需要「要么全成功要么不动」，应先自行校验，或写入一个临时
/// `HeaderMap` 再整体 extend。
///
/// # Arguments
/// * `headers` - Mutable reference to HeaderMap
/// * `values` - 待写入的 (名称, 值) 序列
///
/// # Returns
/// * `Result<()>` - 全部写入成功，或首个非法条目对应的错误
pub fn insert_headers<K, V>(
    headers: &mut HeaderMap<HeaderValue>,
    values: impl IntoIterator<Item = (K, V)>,
) -> Result<()>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    for (name, value) in values {
        let name = name.as_ref();
        let value = value.as_ref();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        // 名称与值都构造成功才 insert：保证单条目不会留下半写状态
        let name = HeaderName::try_from(name).context(InvalidHeaderNameSnafu)?;
        let value = HeaderValue::try_from(value).context(InvalidHeaderValueSnafu)?;
        headers.insert(name, value);
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
    if headers.contains_key(name) {
        return Ok(());
    }
    let values = [(name.to_string(), value.to_string())];
    insert_headers(headers, values)
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
pub fn get_header_value<'a>(headers: &'a HeaderMap<HeaderValue>, key: &str) -> Option<&'a str> {
    headers.get(key).and_then(|value| value.to_str().ok())
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
    let bytes = body.collect().await.context(AxumSnafu)?.to_bytes();
    Ok(bytes)
}

// Name of the device ID cookie
const DEVICE_ID_NAME: &str = "device";
const DEVICE_ID_LIFETIME: Duration = Duration::from_secs(365 * 24 * 60 * 60); // ~52 weeks

/// Retrieves device ID from cookies
///
/// Returns empty string if device cookie is not present
///
/// # Arguments
/// * `jar` - Reference to CookieJar
///
/// # Returns
/// * String containing device ID or empty string
pub fn get_device_id_from_cookie(jar: &CookieJar) -> Option<&str> {
    jar.get(DEVICE_ID_NAME).map(|cookie| cookie.value())
}

/// 生成新的设备 ID Cookie。
///
/// 属性与 `tibba-session` 的会话 Cookie 保持一致：
/// - 52 周有效期、根路径
/// - `HttpOnly`：禁止脚本读取，设备标识不进 XSS 的可窃取面
/// - `SameSite=Lax`：跨站请求不携带，缓解借设备 ID 做的跨站追踪 / CSRF 关联；
///   顶层导航仍会带上，不影响正常回访识别
/// - `Secure`：生产环境强制，禁止设备标识经明文 HTTP 传输被中间人捕获；
///   dev 环境放行以便本地 http 调试
///
/// 返回 `CookieBuilder` 而非 `Cookie`，调用方仍可按需覆盖上述默认值。
pub fn generate_device_id_cookie() -> CookieBuilder<'static> {
    let expires = cookie::time::OffsetDateTime::now_utc()
        .saturating_add(cookie::time::Duration::try_from(DEVICE_ID_LIFETIME).unwrap_or_default());
    Cookie::build((DEVICE_ID_NAME, nanoid!(16)))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(!is_development())
        .expires(expires)
        .path("/")
}
