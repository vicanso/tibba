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

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use snafu::Snafu;
use std::net::{IpAddr, SocketAddr};
use tibba_error::Error as BaseError;

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:middleware=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:middleware";

#[derive(Debug, Snafu)]
pub enum Error {
    /// 验证码校验失败（缺失、过期或不匹配）。
    #[snafu(display("{message}"))]
    Captcha { message: String },
    /// 并发请求或频次超限，对应 HTTP 429。
    #[snafu(display("too many requests, limit: {limit}, current: {current}"))]
    TooManyRequests { limit: i64, current: i64 },
    /// 请求头转字符串失败（非 ASCII / 控制字符）。
    #[snafu(display("{source}"))]
    HeaderValue {
        source: axum::http::header::ToStrError,
    },
    /// CSRF cookie 缺失（首次访问 / 客户端未先取 token）。HTTP 403。
    #[snafu(display("csrf cookie missing"))]
    CsrfCookieMissing,
    /// CSRF 请求头缺失（未把 token 放进 X-CSRF-Token）。HTTP 403。
    #[snafu(display("csrf header missing"))]
    CsrfHeaderMissing,
    /// CSRF cookie 与 header token 不一致，疑似伪造请求。HTTP 403。
    #[snafu(display("csrf token mismatch"))]
    CsrfMismatch,
    /// IP 速率限制触发。HTTP 429。
    #[snafu(display("rate limited (quota: {quota})"))]
    RateLimited { quota: String },
    /// 响应 body 超过 idempotency 缓存上限，无法安全缓存。HTTP 503。
    #[snafu(display("idempotency body too large (limit {limit_bytes} bytes)"))]
    IdempotencyBodyTooLarge { limit_bytes: usize },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Captcha { message } => BaseError::new(message).with_sub_category("captcha"),
            Error::TooManyRequests { limit, current } => BaseError::new(format!(
                "too many requests, limit: {limit}, current: {current}"
            ))
            .with_sub_category("too_many_requests")
            .with_status(429),
            Error::HeaderValue { source } => {
                BaseError::new(source).with_sub_category("header_value")
            }
            // 三种 CSRF 失败统一 403，sub_category 区分具体原因方便排障
            Error::CsrfCookieMissing => BaseError::new("csrf cookie missing")
                .with_sub_category("csrf_cookie_missing")
                .with_status(403)
                .with_exception(false),
            Error::CsrfHeaderMissing => BaseError::new("csrf header missing")
                .with_sub_category("csrf_header_missing")
                .with_status(403)
                .with_exception(false),
            Error::CsrfMismatch => BaseError::new("csrf token mismatch")
                .with_sub_category("csrf_mismatch")
                .with_status(403)
                .with_exception(false),
            Error::RateLimited { quota } => {
                BaseError::new(format!("rate limited (quota: {quota})"))
                    .with_sub_category("rate_limited")
                    .with_status(429)
                    .with_exception(false)
            }
            Error::IdempotencyBodyTooLarge { limit_bytes } => BaseError::new(format!(
                "idempotency body too large (limit {limit_bytes} bytes)"
            ))
            .with_sub_category("idempotency_body_too_large")
            .with_status(503)
            .with_exception(true),
        };
        err.with_category("middleware")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Sync,
{
    type Rejection = tibba_error::Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        // 直连对端 IP（TCP 层，客户端无法伪造）
        let peer_ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip());

        // 仅当直连对端是可信来源（回环 / 私网，通常是自己的反向代理 / ingress）时，才采信
        // 客户端可伪造的 X-Forwarded-For / X-Real-Ip；直连来自公网时一律用对端 IP，
        // 防止攻击者伪造转发头绕过基于 IP 的限流 / 暴力破解锁定 / 审计记录。
        let client_ip = if peer_ip.map(is_trusted_proxy).unwrap_or(false) {
            parts
                .headers
                .get("X-Forwarded-For")
                .and_then(|header| header.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim())
                .and_then(|s| s.parse::<IpAddr>().ok())
                .or_else(|| {
                    parts
                        .headers
                        .get("X-Real-Ip")
                        .and_then(|header| header.to_str().ok())
                        .map(|s| s.trim())
                        .and_then(|s| s.parse::<IpAddr>().ok())
                })
                .or(peer_ip)
        } else {
            peer_ip
        };

        // if all attempts fail (result is None), return error
        client_ip
            .map(ClientIp)
            .ok_or_else(|| BaseError::new("Client IP address could not be determined"))
    }
}

/// 判断直连对端是否为可信反向代理：回环或私网地址视为可信（自有基础设施），
/// 才据其转发头解析真实客户端 IP。公网对端返回 false，不采信任何转发头。
///
/// IPv6 的 ULA（fc00::/7）与 link-local（fe80::/10）用前缀手判，兼容 MSRV 1.83
/// （`Ipv6Addr::is_unique_local` 等在更高版本才稳定）。
fn is_trusted_proxy(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                || (first & 0xfe00) == 0xfc00 // fc00::/7 唯一本地地址
                || (first & 0xffc0) == 0xfe80 // fe80::/10 链路本地
        }
    }
}

mod common;
mod cors;
mod csrf;
mod entry;
mod http_cache;
mod idempotency;
mod limit;
mod rate_limit;
mod request_id;
mod security_headers;
mod stack;
mod stats;
mod trace;
mod tracker;

pub use common::*;
pub use cors::*;
pub use csrf::*;
pub use entry::*;
pub use http_cache::*;
pub use idempotency::*;
pub use limit::*;
pub use rate_limit::*;
pub use request_id::*;
pub use security_headers::*;
pub use stack::*;
pub use stats::*;
pub use trace::*;
pub use tracker::*;

#[cfg(test)]
mod tests {
    use super::is_trusted_proxy;
    use std::net::IpAddr;

    #[test]
    fn trusted_proxy_recognizes_private_and_loopback() {
        // 可信：回环 / 私网 / 链路本地
        for ip in [
            "127.0.0.1",
            "::1",
            "10.1.2.3",
            "172.16.5.6",
            "192.168.0.1",
            "169.254.1.1",
            "fd00::1", // ULA
            "fe80::1", // link-local
        ] {
            let parsed: IpAddr = ip.parse().unwrap();
            assert!(is_trusted_proxy(parsed), "{ip} 应视为可信对端");
        }
    }

    #[test]
    fn public_peer_is_not_trusted() {
        // 公网对端：不采信转发头
        for ip in ["1.1.1.1", "8.8.8.8", "203.0.113.7", "2606:4700::1111"] {
            let parsed: IpAddr = ip.parse().unwrap();
            assert!(!is_trusted_proxy(parsed), "{ip} 不应视为可信对端");
        }
    }
}
