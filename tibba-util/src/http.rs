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

use super::{Error, InvalidHeaderNameSnafu, InvalidHeaderValueSnafu, is_development};
use axum::http::{HeaderMap, HeaderValue, header, header::HeaderName};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use cookie::{CookieBuilder, SameSite};
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
///
/// 本函数在**每个响应**上被安全头、trace 头等中间件调用多次。此前它先把
/// `name` / `value` 各 `to_string()` 一次再交给 [`insert_headers`]——后者本就
/// 接受 `AsRef<str>`，这两次堆分配纯属多余。
///
/// 名称与值在编译期已知的调用方，更好的做法是预先构造好 `HeaderName` /
/// `HeaderValue`，直接 `headers.entry(name).or_insert(value)`，连解析都省掉。
pub fn set_header_if_not_exist(
    headers: &mut HeaderMap<HeaderValue>,
    name: &str,
    value: &str,
) -> Result<()> {
    if headers.contains_key(name) {
        return Ok(());
    }
    insert_headers(headers, [(name, value)])
}

/// 响应尚未设置 `Cache-Control` 时补上 `no-cache`。
///
/// 每个响应都会走到这里，故直接用类型化的 `HeaderName` / `HeaderValue` 走
/// `entry().or_insert()`：不解析字符串、不分配、也不存在失败分支（此前是经由
/// 字符串版 helper 再 `let _ =` 吞掉一个永远不会发生的错误）。
pub fn set_no_cache_if_not_exist(headers: &mut HeaderMap<HeaderValue>) {
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-cache"));
}

/// 读取请求头的字符串值；不存在或不是合法可见 ASCII 时返回 `None`。
pub fn get_header_value<'a>(headers: &'a HeaderMap<HeaderValue>, key: &str) -> Option<&'a str> {
    headers.get(key).and_then(|value| value.to_str().ok())
}

/// 设备 ID cookie 名。
const DEVICE_ID_NAME: &str = "device";
/// 设备 ID 有效期：约 52 周。
const DEVICE_ID_LIFETIME: Duration = Duration::from_secs(365 * 24 * 60 * 60);
/// 设备 ID 长度，与 [`generate_device_id_cookie`] 生成的一致。
const DEVICE_ID_LEN: usize = 16;

/// 是否是本服务签发的设备 ID 形态：定长、nanoid 默认字母表（`A-Za-z0-9_-`）。
fn is_valid_device_id(value: &str) -> bool {
    value.len() == DEVICE_ID_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 从 cookie 取设备 ID；不存在或**不是本服务签发的形态**时返回 `None`。
///
/// cookie 完全由客户端控制，而设备 ID 会进入每个请求的上下文与日志。此前
/// 原样透传，一个 4 KB 的 cookie 值就会被写进每一行日志；含换行等控制字符的
/// 值（浏览器会拒绝，但 curl 不会）还能伪造日志行。只认我们自己生成的格式，
/// 其余视为没有设备 ID。
pub fn get_device_id_from_cookie(jar: &CookieJar) -> Option<&str> {
    jar.get(DEVICE_ID_NAME)
        .map(|cookie| cookie.value())
        .filter(|value| is_valid_device_id(value))
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
    Cookie::build((DEVICE_ID_NAME, nanoid!(DEVICE_ID_LEN)))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(!is_development())
        .expires(expires)
        .path("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn no_cache_is_added_only_when_absent() {
        let mut headers = HeaderMap::new();
        set_no_cache_if_not_exist(&mut headers);
        assert_eq!(headers[header::CACHE_CONTROL], "no-cache");

        // handler 已设置的值优先，不得覆盖
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("max-age=60"),
        );
        set_no_cache_if_not_exist(&mut headers);
        assert_eq!(headers[header::CACHE_CONTROL], "max-age=60");
    }

    #[test]
    fn set_header_if_not_exist_respects_existing_value() {
        let mut headers = HeaderMap::new();
        set_header_if_not_exist(&mut headers, "X-Trace-Id", "a").expect("合法头");
        set_header_if_not_exist(&mut headers, "X-Trace-Id", "b").expect("合法头");
        assert_eq!(headers["x-trace-id"], "a");
        // 非法名称返回错误，且不留下半写状态
        assert!(set_header_if_not_exist(&mut headers, "bad name", "v").is_err());
        assert_eq!(headers.len(), 1);
    }

    fn jar_with_device(value: &str) -> CookieJar {
        CookieJar::new().add(Cookie::new(DEVICE_ID_NAME, value.to_string()))
    }

    /// 自己签发的设备 ID 必须能取回——校验不能误伤正常路径。
    #[test]
    fn generated_device_id_round_trips() {
        let cookie = generate_device_id_cookie().build();
        let jar = jar_with_device(cookie.value());
        assert_eq!(get_device_id_from_cookie(&jar), Some(cookie.value()));
    }

    /// **回归守卫**：非本服务签发形态的值一律视为没有设备 ID。
    ///
    /// 此前原样透传，超长值会被写进每一行日志。
    #[test]
    fn foreign_device_ids_are_rejected() {
        for bad in [
            "",
            "short",
            &"a".repeat(DEVICE_ID_LEN + 1),
            &"a".repeat(4096),
            "abcdefgh ijklmno", // 空格
            "abcdefgh.ijklmno", // 字母表外字符
            "设备设备设备设",   // 非 ASCII
        ] {
            assert_eq!(
                get_device_id_from_cookie(&jar_with_device(bad)),
                None,
                "{bad:?}"
            );
        }
        assert_eq!(get_device_id_from_cookie(&CookieJar::new()), None);
    }
}
