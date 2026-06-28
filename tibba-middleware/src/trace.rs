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

//! 分布式追踪中间件 —— 为每个入站请求创建 OpenTelemetry 服务端 span。
//!
//! ## 行为
//! 1. 按 W3C Trace Context 规范从请求头提取上游 `traceparent` / `tracestate`，
//!    作为本次 span 的父上下文，使跨服务调用归入同一条 trace。
//! 2. 创建 `http_request` span（携带 method / path / 响应状态码等语义字段），
//!    在其内执行后续中间件与 handler；span 经 `tracing-opentelemetry` 导出到 OTLP。
//! 3. 上游未启用 OTel（未设全局 propagator / 未挂 OTLP layer）时，提取与导出
//!    均为 no-op，仅多创建一个轻量 tracing span，开销可忽略。
//!
//! ## 用法
//! 全局挂在 `request_id` 之后、业务路由之前：
//!
//! ```ignore
//! use tibba_middleware::otel_trace;
//! use axum::middleware::from_fn;
//!
//! Router::new()
//!     .merge(api_router)
//!     .layer(from_fn(otel_trace))
//! ```

use crate::LOG_TARGET;
use axum::extract::Request;
use axum::http::HeaderMap;
use axum::http::header::HeaderName;
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::propagation::Extractor;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// 从 `HeaderMap` 读取 OpenTelemetry 上下文的适配器，供全局 propagator 提取
/// 上游 W3C `traceparent` / `tracestate` 等追踪头。
struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    /// 按 header 名取值；非 ASCII / 含控制字符时返回 None。
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    /// 返回全部 header 名，供 propagator 遍历查找追踪字段。
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(HeaderName::as_str).collect()
    }
}

/// axum 中间件：为入站请求建立分布式追踪 span。
///
/// **不会失败** —— 提取失败或未启用 OTel 时退化为根 span / no-op，始终放行请求。
pub async fn otel_trace(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    // 提取上游 trace 上下文（无则为空，本 span 即成为根 span）
    let parent_cx = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(req.headers()))
    });

    // 按 OTel HTTP 语义约定命名与打标；生产如需控制基数，可改用匹配到的路由模板
    // （而非原始 path）作为 otel.name，避免高基数 ID 路径撑爆 trace 后端
    let span = tracing::info_span!(
        "http_request",
        otel.name = %format!("{method} {path}"),
        otel.kind = "server",
        http.request.method = %method,
        url.path = %path,
        http.response.status_code = tracing::field::Empty,
    );
    // 设置父上下文为 best-effort：失败（极罕见，仅上下文异常时）只记 debug，不影响请求
    if let Err(err) = span.set_parent(parent_cx) {
        tracing::debug!(target: LOG_TARGET, error = ?err, "set parent trace context failed");
    }

    async move {
        let response = next.run(req).await;
        tracing::Span::current().record(
            "http.response.status_code",
            u64::from(response.status().as_u16()),
        );
        response
    }
    .instrument(span)
    .await
}
