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
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, ETAG, IF_NONE_MATCH};
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

    /// 把 `Cache-Control` 补进响应（已存在不覆盖；空配置跳过）。
    fn apply_cache_control(&self, headers: &mut HeaderMap) {
        if !self.cache_control.is_empty() {
            let _ = set_header_if_not_exist(headers, CACHE_CONTROL.as_str(), &self.cache_control);
        }
    }
}

/// axum 中间件：为 GET 响应生成 ETag 并处理 304。
pub async fn http_cache(State(config): State<HttpCache>, req: Request, next: Next) -> Response {
    // 仅对 GET 生效
    if req.method() != Method::GET {
        return next.run(req).await;
    }
    // 先取出请求侧的 If-None-Match，再消费 req
    let if_none_match = req
        .headers()
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let res = next.run(req).await;

    // 仅处理 2xx；非 2xx 或 handler 自带 ETag → 直通
    if !res.status().is_success() || res.headers().contains_key(ETAG) {
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

    // 命中 If-None-Match → 304（空 body，仅回 ETag + Cache-Control）
    if if_none_match
        .as_deref()
        .is_some_and(|inm| etag_matches(inm, &etag))
    {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = StatusCode::NOT_MODIFIED;
        if let Ok(value) = HeaderValue::from_str(&etag) {
            res.headers_mut().insert(ETAG, value);
        }
        config.apply_cache_control(res.headers_mut());
        return res;
    }

    // 未命中 → 原响应 + ETag + Cache-Control
    if let Ok(value) = HeaderValue::from_str(&etag) {
        parts.headers.insert(ETAG, value);
    }
    config.apply_cache_control(&mut parts.headers);
    Response::from_parts(parts, Body::from(bytes))
}

/// 由响应体计算弱校验 ETag：`W/"<len>-<hash>"`。
fn compute_etag(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("W/\"{:x}-{:x}\"", bytes.len(), hasher.finish())
}

/// `If-None-Match` 是否匹配给定 ETag。支持逗号分隔的多值与 `*`；
/// 比对时去掉弱校验前缀 `W/`（RFC 规定条件 GET 用弱比较）。
fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    let normalize = |s: &str| s.trim().trim_start_matches("W/").trim().to_string();
    let target = normalize(etag);
    if_none_match.split(',').any(|entry| {
        let entry = entry.trim();
        entry == "*" || normalize(entry) == target
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
