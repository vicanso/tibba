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

//! HTTP 缓存中间件 —— 为 GET 响应自动生成 ETag 并处理条件请求（`If-None-Match` → 304）。
//!
//! ## 行为
//! - 仅作用于 `GET`；非 GET 直通
//! - 仅处理 **2xx** 且 **handler 未自带 ETag** 的响应（自带 ETag 视为 handler 自管，直通）
//! - 仅在响应 `Content-Length` 已知且不超过上限时介入：避免缓冲流式 / 超大响应。
//!   未知长度（chunked / SSE）一律直通，**绝不**因缓存优化而吞掉响应体
//! - 命中 `If-None-Match` → 返回 `304 Not Modified`（空 body，仅回 ETag + Cache-Control）
//! - 未命中 → 原样返回，附加 `ETag` 与（不覆盖 handler 的）`Cache-Control`
//!
//! ## ETag 取值
//! 弱校验 ETag `W/"<len>-<hash>"`，hash 由 `std` 的 `DefaultHasher`（SipHash）对响应体计算。
//! 非加密哈希足够做缓存校验；弱校验语义（`W/`）也契合「按内容等价」而非「字节完全一致」。
//!
//! ## 与压缩的关系
//! 应挂在压缩层**内侧**：本中间件对未压缩的响应体算 ETag，压缩层在更外层再压缩，
//! 两侧表示一致，客户端下次带 `If-None-Match` 比对即可命中。
//!
//! ## 用法
//! ```ignore
//! use tibba_middleware::{http_cache, HttpCache};
//! use axum::middleware::from_fn_with_state;
//!
//! let config = HttpCache::default().with_cache_control("public, max-age=60");
//! Router::new().merge(api_router).layer(from_fn_with_state(config, http_cache));
//! ```

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{
    AUTHORIZATION, CACHE_CONTROL, CONTENT_LENGTH, COOKIE, ETAG, IF_NONE_MATCH, SET_COOKIE, VARY,
};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::hash::{Hash, Hasher};
use tibba_util::set_header_if_not_exist;

/// 缓冲并计算 ETag 的响应体上限，超过则直通（不介入）。
const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024; // 1 MB
/// 默认 `Cache-Control`：`no-cache` 表示「可缓存但每次需带 ETag 回源校验」，
/// 配合 304 在内容不变时省下响应体传输，又保证不会拿到过期数据。
const DEFAULT_CACHE_CONTROL: &str = "no-cache";

/// HTTP 缓存配置。字段私有，链式 `with_xxx` 设置。
#[derive(Debug, Clone)]
pub struct HttpCache {
    /// 注入的 `Cache-Control` 值（handler 已设则不覆盖）；空串表示不注入。
    cache_control: String,
    /// 介入的响应体大小上限（字节）。
    max_body_bytes: usize,
}

impl Default for HttpCache {
    fn default() -> Self {
        Self {
            cache_control: DEFAULT_CACHE_CONTROL.to_string(),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }
}

impl HttpCache {
    /// 以默认基线创建。等价于 [`HttpCache::default`]。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置注入的 `Cache-Control`（如 `public, max-age=60`）。传空串关闭注入。
    #[must_use]
    pub fn with_cache_control(mut self, value: impl Into<String>) -> Self {
        self.cache_control = value.into();
        self
    }

    /// 设置介入的响应体上限（字节）。
    #[must_use]
    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Self {
        self.max_body_bytes = max_body_bytes;
        self
    }

    /// 把 `Cache-Control` 补进响应；若请求带有认证上下文（Cookie / Authorization），
    /// 禁止输出 `public`，降级为 `private, no-cache`。
    fn apply_cache_headers(&self, headers: &mut HeaderMap, authenticated: bool) {
        if authenticated {
            let existing_is_public = headers
                .get(CACHE_CONTROL)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|s| s.to_ascii_lowercase().contains("public"));
            if !headers.contains_key(CACHE_CONTROL) || existing_is_public {
                headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
            }
            append_vary_token(headers, "Accept-Encoding");
            append_vary_token(headers, "Authorization");
            append_vary_token(headers, "Cookie");
        } else {
            if !self.cache_control.is_empty() {
                let _ =
                    set_header_if_not_exist(headers, CACHE_CONTROL.as_str(), &self.cache_control);
            }
            append_vary_token(headers, "Accept-Encoding");
        }
    }
}

/// 将 `token` 并入 `Vary` 响应头（大小写不敏感去重，不覆盖已有 `Accept-Language` 等字段）。
fn append_vary_token(headers: &mut HeaderMap, token: &str) {
    if let Some(existing) = headers.get(VARY).and_then(|v| v.to_str().ok()) {
        let already = existing
            .split(',')
            .any(|part| part.trim() == "*" || part.trim().eq_ignore_ascii_case(token));
        if already {
            return;
        }
        let merged = if existing.trim().is_empty() {
            token.to_string()
        } else {
            format!("{existing}, {token}")
        };
        if let Ok(val) = HeaderValue::from_str(&merged) {
            headers.insert(VARY, val);
        }
    } else if let Ok(val) = HeaderValue::from_str(token) {
        headers.insert(VARY, val);
    }
}

/// axum 中间件：为 GET 响应生成 ETag 并处理 304。
pub async fn http_cache(State(config): State<HttpCache>, req: Request, next: Next) -> Response {
    // 仅对 GET 生效
    if req.method() != Method::GET {
        return next.run(req).await;
    }
    let authenticated =
        req.headers().contains_key(AUTHORIZATION) || req.headers().contains_key(COOKIE);
    // 先取出请求侧的 If-None-Match，再消费 req
    let if_none_match = req
        .headers()
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let res = next.run(req).await;

    // 仅处理 2xx；非 2xx、handler 自带 ETag、带 Set-Cookie 或声明 no-store → 直通
    let is_no_store = res
        .headers()
        .get(CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.to_ascii_lowercase().contains("no-store"));
    if !res.status().is_success()
        || res.headers().contains_key(ETAG)
        || res.headers().contains_key(SET_COOKIE)
        || is_no_store
    {
        return res;
    }

    // 仅在 Content-Length 已知且不超上限时缓冲；否则直通（不吞流式 / 超大响应体）
    let within_limit = res
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .is_some_and(|len| len <= config.max_body_bytes);
    if !within_limit {
        return res;
    }

    let (mut parts, body) = res.into_parts();
    // Content-Length 已确认 ≤ 上限，这里读取不会触发 limit 错误；万一异常则放弃介入
    let Ok(bytes) = axum::body::to_bytes(body, config.max_body_bytes).await else {
        // body 已被消费且读取失败：只能返回一个错误响应（极少触发）
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    let etag = compute_etag(&bytes);
    config.apply_cache_headers(&mut parts.headers, authenticated);

    // 命中 If-None-Match → 304（空 body，保留 ETag + Cache-Control + Vary）
    if if_none_match
        .as_deref()
        .is_some_and(|inm| etag_matches(inm, &etag))
    {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = StatusCode::NOT_MODIFIED;
        if let Ok(value) = HeaderValue::from_str(&etag) {
            res.headers_mut().insert(ETAG, value);
        }
        if let Some(cc) = parts.headers.get(CACHE_CONTROL) {
            res.headers_mut().insert(CACHE_CONTROL, cc.clone());
        }
        if let Some(vary) = parts.headers.get(VARY) {
            res.headers_mut().insert(VARY, vary.clone());
        }
        return res;
    }

    // 未命中 → 原响应 + ETag + Cache-Control + Vary
    if let Ok(value) = HeaderValue::from_str(&etag) {
        parts.headers.insert(ETAG, value);
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// 由响应体计算弱校验 ETag：`W/"<len>-<hash>"`。
fn compute_etag(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("W/\"{:x}-{:x}\"", bytes.len(), hasher.finish())
}

/// 零分配去掉弱校验前缀 `W/`。
fn strip_weak_prefix(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix("W/").unwrap_or(s).trim()
}

/// `If-None-Match` 是否匹配给定 ETag。支持逗号分隔的多值与 `*`；
/// 零分配比对，去掉弱校验前缀 `W/`（RFC 9110 条件 GET 用弱比较）。
fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    let target = strip_weak_prefix(etag);
    if_none_match.split(',').any(|entry| {
        let entry = entry.trim();
        entry == "*" || strip_weak_prefix(entry) == target
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use tower::ServiceExt;

    /// 同内容 ETag 稳定，不同内容 ETag 不同。
    #[test]
    fn etag_is_content_addressed() {
        let a = compute_etag(b"hello");
        let b = compute_etag(b"hello");
        let c = compute_etag(b"world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("W/\""));
    }

    /// If-None-Match 精确命中、弱前缀命中、通配命中、未命中。
    #[test]
    fn if_none_match_semantics() {
        let etag = compute_etag(b"payload");
        // 精确（带 W/）
        assert!(etag_matches(&etag, &etag));
        // 去掉 W/ 前缀也应命中
        let strong = etag.trim_start_matches("W/").to_string();
        assert!(etag_matches(&strong, &etag));
        // 多值列表中其一命中
        let list = format!("\"other\", {etag}");
        assert!(etag_matches(&list, &etag));
        // 通配
        assert!(etag_matches("*", &etag));
        // 未命中
        assert!(!etag_matches("\"nope\"", &etag));
    }

    #[tokio::test]
    async fn set_cookie_response_is_never_tagged_with_etag() {
        let app = Router::new()
            .route(
                "/cookie",
                get(|| async {
                    let mut resp = Response::new(Body::from("ok"));
                    resp.headers_mut()
                        .insert(CONTENT_LENGTH, HeaderValue::from_static("2"));
                    resp.headers_mut()
                        .insert(SET_COOKIE, HeaderValue::from_static("sid=1"));
                    resp
                }),
            )
            .layer(from_fn_with_state(
                HttpCache::new().with_cache_control("public, max-age=60"),
                http_cache,
            ));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/cookie")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert!(!resp.headers().contains_key(ETAG));
        assert!(!resp.headers().contains_key(CACHE_CONTROL));
    }

    #[tokio::test]
    async fn authenticated_request_downgrades_public_and_preserves_304_headers() {
        let app = Router::new()
            .route(
                "/me",
                get(|| async {
                    let mut resp = Response::new(Body::from("hello"));
                    resp.headers_mut()
                        .insert(CONTENT_LENGTH, HeaderValue::from_static("5"));
                    resp.headers_mut()
                        .insert(VARY, HeaderValue::from_static("Accept-Language"));
                    resp
                }),
            )
            .layer(from_fn_with_state(
                HttpCache::new().with_cache_control("public, max-age=60"),
                http_cache,
            ));

        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header(AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .expect("req1"),
            )
            .await
            .expect("resp1");
        assert_eq!(
            first
                .headers()
                .get(CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("private, no-cache")
        );
        let vary = first
            .headers()
            .get(VARY)
            .and_then(|v| v.to_str().ok())
            .expect("Vary present");
        assert!(vary.contains("Accept-Language"), "{vary}");
        assert!(vary.contains("Authorization"), "{vary}");
        assert!(vary.contains("Cookie"), "{vary}");
        let etag = first
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .expect("ETag present")
            .to_string();

        let second = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header(AUTHORIZATION, "Bearer secret")
                    .header(IF_NONE_MATCH, etag.as_str())
                    .body(Body::empty())
                    .expect("req2"),
            )
            .await
            .expect("resp2");
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            second.headers().get(ETAG).and_then(|v| v.to_str().ok()),
            Some(etag.as_str())
        );
        assert_eq!(
            second
                .headers()
                .get(CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("private, no-cache")
        );
        let vary_304 = second
            .headers()
            .get(VARY)
            .and_then(|v| v.to_str().ok())
            .expect("304 Vary present");
        assert!(vary_304.contains("Accept-Language"), "{vary_304}");
        assert!(vary_304.contains("Authorization"), "{vary_304}");
    }
}
