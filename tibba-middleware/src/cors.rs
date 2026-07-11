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

//! CORS 跨域中间件 —— 处理浏览器预检（preflight）与实际请求的跨域响应头。
//!
//! ## 行为
//! - 无 `Origin` 头（同源 / 非浏览器）→ 直通，不加任何 CORS 头
//! - `OPTIONS` + `Access-Control-Request-Method` → 预检：直接返回 `204` 并附 CORS 头，
//!   **不进入业务 handler**（也就跳过了 entry/stats/session 等内层中间件）
//! - 其余跨域请求 → 正常执行后在响应上补 `Access-Control-Allow-Origin` 等头
//!
//! ## 设计取舍
//! - **凭据与通配符互斥**：`Access-Control-Allow-Credentials: true` 时不能用 `*`，
//!   故开启凭据且未配置白名单时，回显请求的具体 `Origin`（而非 `*`）。
//! - **白名单未命中**：不输出 `Access-Control-Allow-Origin`，由浏览器拦截（不强行 403）。
//! - 始终追加 `Vary: Origin`，避免 CDN / 浏览器把某来源的响应错误复用给另一来源。
//!
//! ## 用法
//! ```ignore
//! use tibba_middleware::{cors, Cors};
//! use axum::middleware::from_fn_with_state;
//! use std::sync::Arc;
//!
//! // 生产：显式白名单 + 凭据
//! let config = Cors::new()
//!     .add_allow_origin("https://app.example.com")
//!     .with_allow_credentials(true);
//! Router::new()
//!     .merge(api_router)
//!     .layer(from_fn_with_state(Arc::new(config), cors));
//! ```

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_EXPOSE_HEADERS, ACCESS_CONTROL_MAX_AGE,
    ACCESS_CONTROL_REQUEST_HEADERS, ACCESS_CONTROL_REQUEST_METHOD, ORIGIN, VARY,
};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;
use std::time::Duration;

/// 默认允许的方法集合。
const DEFAULT_ALLOW_METHODS: &str = "GET,POST,PUT,PATCH,DELETE,OPTIONS";
/// 默认允许的请求头（覆盖本项目实际用到的鉴权 / 幂等 / CSRF 头）。
const DEFAULT_ALLOW_HEADERS: &str =
    "Content-Type,Authorization,X-CSRF-Token,X-API-Key,Idempotency-Key";
/// 默认暴露给前端 JS 读取的响应头。
const DEFAULT_EXPOSE_HEADERS: &str = "X-Request-Id";
/// 预检结果默认缓存时长（浏览器在此期间不再重复 preflight）。
const DEFAULT_MAX_AGE: Duration = Duration::from_secs(86_400);

/// CORS 配置。
///
/// 字段全部私有，只能通过链式 `with_xxx` / `add_xxx` 设置。[`Default`] 给出
/// **任意来源、无凭据**的宽松基线，便于本地起步；生产应通过 [`Cors::add_allow_origin`]
/// 收敛来源，并按需 [`Cors::with_allow_credentials`] 开启凭据。
#[derive(Debug, Clone)]
pub struct Cors {
    /// 允许的来源白名单；**空表示任意来源**。
    allow_origins: Vec<String>,
    /// `Access-Control-Allow-Methods` 值。
    allow_methods: String,
    /// `Access-Control-Allow-Headers` 值；空则在预检时回显请求所带的头清单。
    allow_headers: String,
    /// `Access-Control-Expose-Headers` 值；空则不输出该头。
    expose_headers: String,
    /// 是否允许携带凭据（cookie / Authorization）。
    allow_credentials: bool,
    /// 预检缓存时长。
    max_age: Duration,
}

impl Default for Cors {
    fn default() -> Self {
        Self {
            allow_origins: Vec::new(),
            allow_methods: DEFAULT_ALLOW_METHODS.to_string(),
            allow_headers: DEFAULT_ALLOW_HEADERS.to_string(),
            expose_headers: DEFAULT_EXPOSE_HEADERS.to_string(),
            allow_credentials: false,
            max_age: DEFAULT_MAX_AGE,
        }
    }
}

impl Cors {
    /// 以宽松基线创建配置。等价于 [`Cors::default`]。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一个允许的来源（如 `https://app.example.com`）。可多次调用。
    #[must_use]
    pub fn add_allow_origin(mut self, origin: impl Into<String>) -> Self {
        self.allow_origins.push(origin.into());
        self
    }

    /// 设置允许的方法（逗号分隔，如 `GET,POST`）。
    #[must_use]
    pub fn with_allow_methods(mut self, methods: impl Into<String>) -> Self {
        self.allow_methods = methods.into();
        self
    }

    /// 设置允许的请求头（逗号分隔）。传空串表示预检时回显客户端请求的头清单。
    #[must_use]
    pub fn with_allow_headers(mut self, headers: impl Into<String>) -> Self {
        self.allow_headers = headers.into();
        self
    }

    /// 设置暴露给前端 JS 读取的响应头（逗号分隔）。传空串关闭该头。
    #[must_use]
    pub fn with_expose_headers(mut self, headers: impl Into<String>) -> Self {
        self.expose_headers = headers.into();
        self
    }

    /// 设置是否允许携带凭据。开启后通配符 `*` 失效，将回显具体来源。
    #[must_use]
    pub fn with_allow_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }

    /// 设置预检缓存时长。
    #[must_use]
    pub fn with_max_age(mut self, max_age: Duration) -> Self {
        self.max_age = max_age;
        self
    }

    /// 当前是否「任意来源」（白名单为空）。
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.allow_origins.is_empty()
    }

    /// 是否允许携带凭据。
    #[must_use]
    pub fn allow_credentials(&self) -> bool {
        self.allow_credentials
    }

    /// 已配置的来源白名单（只读）。
    #[must_use]
    pub fn allow_origins(&self) -> &[String] {
        &self.allow_origins
    }

    /// 生产环境安全校验：禁止「任意来源」白名单为空。
    ///
    /// 宽松默认值只适合本地；生产必须 `add_allow_origin` 收敛，否则启动失败（fail-fast）。
    pub fn assert_production_safe(&self) -> Result<(), String> {
        if self.is_open() {
            return Err(
                "production CORS must set allow_origins (empty whitelist = reflect any Origin); \
                 set basic.cors_allow_origins or TIBBA_WEB__BASIC__CORS_ALLOW_ORIGINS"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// 依据请求 `Origin` 解析应回显的 `Access-Control-Allow-Origin` 值。
    /// 返回 `None` 表示该来源不被允许（不输出该头，浏览器据此拦截）。
    fn resolve_origin(&self, origin: Option<&str>) -> Option<HeaderValue> {
        if self.allow_origins.is_empty() {
            // 任意来源：带凭据必须回显具体来源（`*` 与凭据互斥），否则用 `*`
            if self.allow_credentials {
                origin.and_then(|o| HeaderValue::from_str(o).ok())
            } else {
                Some(HeaderValue::from_static("*"))
            }
        } else {
            // 白名单：命中才回显
            let origin = origin?;
            self.allow_origins
                .iter()
                .any(|allowed| allowed == origin)
                .then(|| HeaderValue::from_str(origin).ok())
                .flatten()
        }
    }

    /// 写入预检响应头（OPTIONS 204）。`allow_origin` 为 `None` 时不输出任何 CORS 头。
    fn apply_preflight(
        &self,
        headers: &mut HeaderMap,
        allow_origin: Option<&HeaderValue>,
        req_headers: &HeaderMap,
    ) {
        // Vary: Origin 始终追加，避免缓存把不同来源的预检结果串用
        headers.append(VARY, HeaderValue::from_static("Origin"));
        let Some(origin) = allow_origin else {
            return;
        };
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
        if let Ok(value) = HeaderValue::from_str(&self.allow_methods) {
            headers.insert(ACCESS_CONTROL_ALLOW_METHODS, value);
        }
        // 配置了固定头清单就用它；否则回显客户端请求的头清单
        let allow_headers = if self.allow_headers.is_empty() {
            req_headers.get(ACCESS_CONTROL_REQUEST_HEADERS).cloned()
        } else {
            HeaderValue::from_str(&self.allow_headers).ok()
        };
        if let Some(value) = allow_headers {
            headers.insert(ACCESS_CONTROL_ALLOW_HEADERS, value);
        }
        if let Ok(value) = HeaderValue::from_str(&self.max_age.as_secs().to_string()) {
            headers.insert(ACCESS_CONTROL_MAX_AGE, value);
        }
        if self.allow_credentials {
            headers.insert(
                ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
    }

    /// 写入实际请求的跨域响应头。`allow_origin` 为 `None` 时不输出任何 CORS 头。
    fn apply_actual(&self, headers: &mut HeaderMap, allow_origin: Option<&HeaderValue>) {
        headers.append(VARY, HeaderValue::from_static("Origin"));
        let Some(origin) = allow_origin else {
            return;
        };
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
        if self.allow_credentials {
            headers.insert(
                ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
        if !self.expose_headers.is_empty()
            && let Ok(value) = HeaderValue::from_str(&self.expose_headers)
        {
            headers.insert(ACCESS_CONTROL_EXPOSE_HEADERS, value);
        }
    }
}

/// axum 中间件：处理 CORS 预检与实际请求。
///
/// **不会失败** —— 预检直接短路返回 204，实际请求仅在响应上补头。
pub async fn cors(State(config): State<Arc<Cors>>, req: Request, next: Next) -> Response {
    // 无 Origin → 同源或非浏览器请求，无需 CORS 处理
    let Some(origin) = req
        .headers()
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
    else {
        return next.run(req).await;
    };

    let allow_origin = config.resolve_origin(Some(&origin));

    // 预检：OPTIONS 且带 Access-Control-Request-Method
    let is_preflight = req.method() == Method::OPTIONS
        && req.headers().contains_key(ACCESS_CONTROL_REQUEST_METHOD);
    if is_preflight {
        let req_headers = req.headers().clone();
        let mut res = Response::new(Body::empty());
        *res.status_mut() = StatusCode::NO_CONTENT;
        config.apply_preflight(res.headers_mut(), allow_origin.as_ref(), &req_headers);
        return res;
    }

    let mut res = next.run(req).await;
    config.apply_actual(res.headers_mut(), allow_origin.as_ref());
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空白名单 + 无凭据 → 回显 `*`。
    #[test]
    fn any_origin_without_credentials_uses_wildcard() {
        let cors = Cors::new();
        let v = cors.resolve_origin(Some("https://a.com")).unwrap();
        assert_eq!(v, "*");
    }

    /// 空白名单 + 带凭据 → 回显具体来源（不能用 `*`）。
    #[test]
    fn any_origin_with_credentials_reflects_origin() {
        let cors = Cors::new().with_allow_credentials(true);
        let v = cors.resolve_origin(Some("https://a.com")).unwrap();
        assert_eq!(v, "https://a.com");
    }

    /// 白名单命中回显、未命中返回 None。
    #[test]
    fn allowlist_match_and_miss() {
        let cors = Cors::new().add_allow_origin("https://ok.com");
        assert_eq!(
            cors.resolve_origin(Some("https://ok.com")).unwrap(),
            "https://ok.com"
        );
        assert!(cors.resolve_origin(Some("https://evil.com")).is_none());
    }

    /// 实际请求响应头：写入 ACAO + Vary，并按配置补凭据头。
    #[test]
    fn apply_actual_sets_headers() {
        let cors = Cors::new()
            .add_allow_origin("https://ok.com")
            .with_allow_credentials(true);
        let allow = cors.resolve_origin(Some("https://ok.com"));
        let mut headers = HeaderMap::new();
        cors.apply_actual(&mut headers, allow.as_ref());
        assert_eq!(
            headers.get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
            "https://ok.com"
        );
        assert_eq!(
            headers.get(ACCESS_CONTROL_ALLOW_CREDENTIALS).unwrap(),
            "true"
        );
        assert_eq!(headers.get(VARY).unwrap(), "Origin");
    }

    /// 未命中白名单的实际请求：不输出 ACAO，但仍追加 Vary。
    #[test]
    fn apply_actual_denied_origin_omits_acao() {
        let cors = Cors::new().add_allow_origin("https://ok.com");
        let allow = cors.resolve_origin(Some("https://evil.com"));
        let mut headers = HeaderMap::new();
        cors.apply_actual(&mut headers, allow.as_ref());
        assert!(headers.get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        assert_eq!(headers.get(VARY).unwrap(), "Origin");
    }
}
