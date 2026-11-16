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

//! 中间件栈装配：统一描述并挂载横切能力，避免应用入口散落重复的 `.layer(...)` 顺序约定。

use crate::{
    Cors, HttpCache, SecurityHeaders, cors, entry, http_cache, i18n, processing_limit, request_id,
    security_headers, stats, validate_csrf,
};
use axum::BoxError;
use axum::Router;
use axum::error_handling::HandleErrorLayer;
use axum::extract::DefaultBodyLimit;
use axum::http::{Method, Uri};
use axum::middleware::{from_fn, from_fn_with_state};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tibba_auth::{SessionParams, session_middleware};
use tibba_cache::RedisCache;
use tibba_runtime::AppState;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};

/// 全局默认请求体上限（2 MiB）。
pub const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;
/// 默认请求超时时间（30s）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// 可选中间件开关（默认与主应用一致：全开）。
#[derive(Debug, Clone)]
pub struct MiddlewareOptions {
    /// OpenTelemetry 入站 span（需启用 `otel` feature；未配置 OTLP endpoint 时仍为 no-op）
    pub otel: bool,
    /// GET ETag / If-None-Match → 304
    pub http_cache: bool,
    /// Accept-Language 错误消息本地化
    pub i18n: bool,
    /// API Key（`X-API-Key` / `Authorization: Bearer tibba_…`）注入 Session
    pub api_key: bool,
    /// Double-submit CSRF（状态变更 + Cookie 会话）
    pub csrf: bool,
}

impl Default for MiddlewareOptions {
    fn default() -> Self {
        Self {
            otel: true,
            http_cache: true,
            i18n: true,
            api_key: true,
            csrf: true,
        }
    }
}

impl MiddlewareOptions {
    /// 全部关闭的可选层（仅保留 entry/stats/session/security 等核心层）。
    #[must_use]
    pub fn minimal() -> Self {
        Self {
            otel: false,
            http_cache: false,
            i18n: false,
            api_key: false,
            csrf: false,
        }
    }

    #[must_use]
    pub fn with_otel(mut self, on: bool) -> Self {
        self.otel = on;
        self
    }

    #[must_use]
    pub fn with_http_cache(mut self, on: bool) -> Self {
        self.http_cache = on;
        self
    }

    #[must_use]
    pub fn with_i18n(mut self, on: bool) -> Self {
        self.i18n = on;
        self
    }

    #[must_use]
    pub fn with_api_key(mut self, on: bool) -> Self {
        self.api_key = on;
        self
    }

    #[must_use]
    pub fn with_csrf(mut self, on: bool) -> Self {
        self.csrf = on;
        self
    }
}

/// 在 Session 中间件内侧追加自定义中间件的钩子（例如 `api_key_auth`）。
pub type PostSessionHook = Box<dyn FnOnce(Router) -> Router>;

/// 组装完整 HTTP 中间件栈所需参数。
pub struct MiddlewareStackParams {
    pub state: &'static AppState,
    pub cache: &'static RedisCache,
    pub session_params: Arc<SessionParams>,
    pub cors: Cors,
    pub security_headers: SecurityHeaders,
    pub timeout: Duration,
    pub body_limit: usize,
    pub http_cache: HttpCache,
    pub options: MiddlewareOptions,
    pub post_session_layer: Option<PostSessionHook>,
}

impl fmt::Debug for MiddlewareStackParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MiddlewareStackParams")
            .field("timeout", &self.timeout)
            .field("body_limit", &self.body_limit)
            .field("options", &self.options)
            .field("has_post_session_layer", &self.post_session_layer.is_some())
            .finish_non_exhaustive()
    }
}

impl MiddlewareStackParams {
    /// 构造默认中间件栈参数，其余选项可通过链式方法覆盖。
    pub fn new(
        state: &'static AppState,
        cache: &'static RedisCache,
        session_params: Arc<SessionParams>,
    ) -> Self {
        Self {
            state,
            cache,
            session_params,
            cors: Cors::default(),
            security_headers: SecurityHeaders::default().with_content_security_policy(
                "object-src 'none'; frame-ancestors 'none'; base-uri 'self'",
            ),
            timeout: DEFAULT_TIMEOUT,
            body_limit: DEFAULT_BODY_LIMIT,
            http_cache: HttpCache::default(),
            options: MiddlewareOptions::default(),
            post_session_layer: None,
        }
    }

    #[must_use]
    pub fn with_cors(mut self, cors: Cors) -> Self {
        self.cors = cors;
        self
    }

    #[must_use]
    pub fn with_security_headers(mut self, security_headers: SecurityHeaders) -> Self {
        self.security_headers = security_headers;
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_body_limit(mut self, body_limit: usize) -> Self {
        self.body_limit = body_limit;
        self
    }

    #[must_use]
    pub fn with_http_cache(mut self, http_cache: HttpCache) -> Self {
        self.http_cache = http_cache;
        self
    }

    #[must_use]
    pub fn with_options(mut self, options: MiddlewareOptions) -> Self {
        self.options = options;
        self
    }

    /// 注册挂载在 `session` 内侧的自定义层（如 API Key / 应用签名认证）。多次调用会按调用顺序串联保留。
    #[must_use]
    pub fn with_post_session_layer(
        mut self,
        hook: impl FnOnce(Router) -> Router + 'static,
    ) -> Self {
        self.post_session_layer = Some(match self.post_session_layer {
            Some(prev) => Box::new(move |router| hook(prev(router))),
            None => Box::new(hook),
        });
        self
    }
}

/// 统一处理超时与未捕获的 tower 错误，转换为标准 `tibba_error::Error` JSON 响应。
pub async fn handle_middleware_error(
    method: Method,
    uri: Uri,
    err: BoxError,
) -> tibba_error::Error {
    tracing::error!(method = method.to_string(), uri = uri.to_string(), err);

    let (message, category, status) = if err.is::<tower::timeout::error::Elapsed>() {
        (
            "Request took too long".to_string(),
            "timeout".to_string(),
            408,
        )
    } else {
        (err.to_string(), "exception".to_string(), 500)
    };

    tibba_error::Error::new(message)
        .with_category(category)
        .with_status(status)
}

/// 按严格由内到外的顺序将标准 HTTP 中间件栈应用到 `Router`。
///
/// 实际请求从外到内的执行顺序：
/// 1. `request_id`
/// 2. `otel_trace`（需 `otel` feature 且 `options.otel == true`）
/// 3. `security_headers`
/// 4. `cors`
/// 5. `CompressionLayer`
/// 6. `i18n`（`options.i18n == true`）
/// 7. `timeout`（超限经 `handle_middleware_error` 转 408）
/// 8. `DefaultBodyLimit`
/// 9. `entry`
/// 10. `stats`
/// 11. `processing_limit`
/// 12. `session`
/// 13. `post_session_layer`（`options.api_key == true` 时挂载，如 `api_key_auth`）
/// 14. `validate_csrf`（`options.csrf == true`）
/// 15. `http_cache`（`options.http_cache == true`）
pub fn apply_middleware_stack(mut app: Router, params: MiddlewareStackParams) -> Router {
    let mw = params.options;
    if mw.http_cache {
        app = app.layer(from_fn_with_state(params.http_cache, http_cache));
    }
    if mw.csrf {
        app = app.layer(from_fn(validate_csrf));
    }
    if mw.api_key
        && let Some(hook) = params.post_session_layer
    {
        app = hook(app);
    }

    app = app
        .layer(from_fn_with_state(
            (params.cache, params.session_params),
            session_middleware,
        ))
        .layer(from_fn_with_state(params.state, processing_limit))
        .layer(from_fn_with_state(params.state, stats))
        .layer(from_fn(entry))
        .layer(DefaultBodyLimit::max(params.body_limit))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_middleware_error))
                .timeout(params.timeout),
        );

    if mw.i18n {
        app = app.layer(from_fn(i18n::i18n));
    }

    let predicate = SizeAbove::new(1024)
        .and(NotForContentType::GRPC)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE);
    app = app
        .layer(CompressionLayer::new().compress_when(predicate))
        .layer(from_fn_with_state(Arc::new(params.cors), cors))
        .layer(from_fn_with_state(
            params.security_headers,
            security_headers,
        ));

    #[cfg(feature = "otel")]
    if mw.otel {
        app = app.layer(from_fn(crate::otel_trace));
    }
    #[cfg(not(feature = "otel"))]
    let _ = mw.otel;

    app.layer(from_fn(request_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_on() {
        let o = MiddlewareOptions::default();
        assert!(o.otel && o.http_cache && o.i18n && o.api_key && o.csrf);
    }

    #[test]
    fn minimal_is_all_off() {
        let o = MiddlewareOptions::minimal();
        assert!(!o.otel && !o.http_cache && !o.i18n && !o.api_key && !o.csrf);
    }

    #[test]
    fn fluent_toggles() {
        let o = MiddlewareOptions::minimal()
            .with_csrf(true)
            .with_api_key(true);
        assert!(o.csrf && o.api_key);
        assert!(!o.otel);
    }
}
