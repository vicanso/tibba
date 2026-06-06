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

//! Request ID 中间件 —— 跨服务请求关联的基石。
//!
//! ## 行为
//! 1. 进入时检查 `X-Request-ID` header：
//!    - 已存在且非空 → **信任并复用**（上游网关 / 调用方已注入，保持链路一致）
//!    - 缺失 / 空 → 服务端生成 UUID 填入
//! 2. 把 ID 写入请求扩展 [`RequestId`]，供 handler 通过 extractor 取用
//! 3. 在响应头写回 `X-Request-ID`，便于客户端排障
//!
//! ## 用法
//! 全局挂在最外层（在 entry / stats / 业务路由之前）：
//!
//! ```ignore
//! use tibba_middleware::request_id;
//! use axum::middleware::from_fn;
//!
//! Router::new()
//!     .merge(api_router)
//!     .layer(from_fn(request_id))   // 最外层，保证所有后续中间件都能拿到 ID
//! ```
//!
//! handler 取用：
//!
//! ```ignore
//! use tibba_middleware::RequestId;
//!
//! async fn handler(RequestId(id): RequestId) -> String {
//!     format!("processing {id}")
//! }
//! ```

use axum::extract::{FromRequestParts, Request};
use axum::http::HeaderValue;
use axum::http::header::HeaderName;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use tibba_error::Error as BaseError;
use tibba_util::uuid;

/// 请求 ID header 名（小写，HTTP 标准建议）。
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// 单次请求的关联 ID。中间件注入，handler 通过 `extract::FromRequestParts` 取用。
/// 内部是 String 因 UUID 即可，也可能是上游传入的任意 ID 格式。
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

impl RequestId {
    /// 内部值的 `&str` 视图，方便日志 / 数据库参数绑定。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// axum 中间件：注入或透传 Request ID。
///
/// **不会失败** —— 即便 header 非法也走 fresh UUID 路径，确保下游永远拿得到 ID。
pub async fn request_id(mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get(REQUEST_ID_HEADER.as_str())
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        // 进一步防御：截断到 128 字符避免日志 / DB 列被异常长输入打爆
        .map(|s| s.chars().take(128).collect::<String>())
        .unwrap_or_else(uuid);

    req.extensions_mut().insert(RequestId(id.clone()));

    let mut resp = next.run(req).await;
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert(REQUEST_ID_HEADER, v);
    }
    resp
}

/// handler 通过 `RequestId` 参数直接取请求 ID。
/// 上游未挂 `request_id` 中间件时返回 500（明确告警，避免静默丢失）。
impl<S> FromRequestParts<S> for RequestId
where
    S: Sync,
{
    type Rejection = BaseError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        parts.extensions.get::<RequestId>().cloned().ok_or_else(|| {
            BaseError::new("request_id middleware not mounted before this handler")
                .with_category("middleware")
                .with_status(500)
                .with_exception(true)
        })
    }
}
