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

//! CSRF 防护：**double-submit cookie** 模式。
//!
//! ## 流程
//! 1. 客户端启动时 `GET /csrf/token` —— 服务端生成随机 UUID，写入名为
//!    `csrf_token` 的 cookie（`SameSite=Strict`、非 `HttpOnly`、生产环境 `Secure`），
//!    同时把 token 返回响应体，便于 JS 直接取用。
//! 2. 后续所有状态变更请求（POST/PUT/PATCH/DELETE）必须在 `X-CSRF-Token`
//!    请求头里携带同一 token；中间件 `validate_csrf` 用常数时间比对 cookie 与 header。
//! 3. 不一致 / 缺失 → HTTP 403。GET/HEAD/OPTIONS 直通，不验证。
//!
//! ## 安全性约束
//! - Cookie 必须 `SameSite=Strict`：浏览器不会在跨站请求中带上，攻击者无法借势。
//! - Cookie 必须**非** `HttpOnly`：前端 JS 需要读取以放进请求头（这是 double-submit
//!   pattern 的核心：能读 cookie 的页面与本站同源，跨站脚本读不到）。
//! - 比对走常数时间，避免按字节短路时序泄漏 token。
//!
//! ## 启用方式
//! 路由层按需挂中间件：
//!
//! ```ignore
//! use tibba_middleware::{validate_csrf, csrf_token};
//! use axum::middleware::from_fn;
//!
//! Router::new()
//!     .route("/csrf/token", get(csrf_token))                    // 公开
//!     .route("/api/users/:id", patch(update_user))
//!         .route_layer(from_fn(validate_csrf))                  // 此分支受保护
//! ```

use crate::{CsrfCookieMissingSnafu, CsrfHeaderMissingSnafu, Error, LOG_TARGET};
use axum::Json;
use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Serialize;
use snafu::OptionExt;
use tibba_error::Error as BaseError;
use tibba_util::{is_development, uuid};
use tracing::debug;

type Result<T, E = BaseError> = std::result::Result<T, E>;

/// CSRF cookie 名（前端 JS 需要按此名读 cookie）。
pub const CSRF_COOKIE_NAME: &str = "csrf_token";
/// 状态变更请求必须带的请求头名。
pub const CSRF_HEADER_NAME: &str = "X-CSRF-Token";

#[derive(Debug, Serialize)]
pub struct CsrfTokenResp {
    pub token: String,
}

/// `GET /csrf/token` handler —— 生成新 token、写 cookie、返回 token。
///
/// 调用方挂在公开路径上（无需鉴权）。前端首次加载时调用一次即可，
/// 后续状态变更请求把 token 放进 `X-CSRF-Token` header。
pub async fn csrf_token(jar: CookieJar) -> (CookieJar, Json<CsrfTokenResp>) {
    let token = uuid();
    let mut cookie = Cookie::new(CSRF_COOKIE_NAME, token.clone());
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Strict);
    // 前端 JS 必须能读到（double-submit 的核心约束）
    cookie.set_http_only(false);
    // 生产环境强制 Secure；dev 模式允许 http 调试
    cookie.set_secure(!is_development());
    (jar.add(cookie), Json(CsrfTokenResp { token }))
}

/// 中间件：校验状态变更请求的 CSRF token。GET/HEAD/OPTIONS 直通。
///
/// 校验失败返回 HTTP 403。调用方按路由分组挂载，不要全局挂——会冲击
/// `/csrf/token`、`/login`、`/register` 等不应受此约束的端点。
pub async fn validate_csrf(jar: CookieJar, req: Request, next: Next) -> Result<Response> {
    // 安全方法直通
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(next.run(req).await);
    }

    let cookie_token = jar
        .get(CSRF_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .context(CsrfCookieMissingSnafu)?;

    let header_token = req
        .headers()
        .get(CSRF_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .context(CsrfHeaderMissingSnafu)?;

    if !constant_time_eq(cookie_token.as_bytes(), header_token.as_bytes()) {
        debug!(target: LOG_TARGET, "csrf token mismatch");
        return Err(Error::CsrfMismatch.into());
    }
    Ok(next.run(req).await)
}

/// 常数时间比较，避免按字节短路泄漏 token。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn constant_time_eq_basic() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }
}
