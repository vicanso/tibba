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

//! 流式 HTTP 响应与 SSE（Server-Sent Events）行级读取器。

use super::request::{ClientConfig, ProcessingGuard, is_idempotent, run_on_done};
use super::{Client, Error, HttpStats, Params};
use bytes::{Bytes, BytesMut};
use futures::Stream;
use futures::StreamExt;
use serde::Serialize;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tibba_util::Stopwatch;

/// 默认单行 SSE 缓冲区上限（16 MiB），防止对端长期不发 `\n` 耗尽内存。
pub const DEFAULT_MAX_SSE_LINE_BYTES: usize = 16 * 1024 * 1024;

/// 装箱的字节流类型，统一封装 `tibba_request::Error` 并持有在途并发配额。
pub type BoxByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send>>;

/// [`Client::execute_stream`] 的返回结果：2xx 暴露字节流，≥400 缓冲有限字节供调用方解析错误。
pub enum StreamOutcome {
    /// 上游返回 2xx，消费 `BoxByteStream` 读取分块响应。
    Stream(BoxByteStream),
    /// 上游返回 ≥400，已受限读取完整错误响应体（未做截断解析）。
    StatusError { status: u16, body: Bytes },
}

impl fmt::Debug for StreamOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stream(_) => f.debug_tuple("Stream").field(&"<stream>").finish(),
            Self::StatusError { status, body } => f
                .debug_struct("StatusError")
                .field("status", status)
                .field("body_len", &body.len())
                .finish(),
        }
    }
}

/// 持有 `ProcessingGuard` 与熔断器引用的流包装器：
/// - 只要流未耗尽或未被 drop，并发配额持续计数；
/// - 流中途出错时立即向熔断器记一次失败。
struct GuardedByteStream {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    _guard: ProcessingGuard,
    config: Arc<ClientConfig>,
    path: String,
}

impl Stream for GuardedByteStream {
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(Some(Err(err))) => {
                if let Some(cb) = &this.config.circuit_breaker {
                    cb.record_failure();
                }
                Poll::Ready(Some(Err(Error::Request {
                    service: this.config.service.clone(),
                    path: this.path.clone(),
                    source: err,
                })))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Client {
    /// 发起流式 HTTP 请求并返回字节流（2xx）或完整错误响应体（≥400）。
    ///
    /// - 共享 [`Client`] 的 SSRF 防护、OTel trace 注入、拦截器 `request`/`on_done`、熔断器与重试逻辑；
    /// - 2xx 时并发计数守卫 [`ProcessingGuard`] 保留至返回流耗尽或被 drop，精确反映活跃长连接占用。
    pub async fn execute_stream<Q, P>(
        &self,
        params: Params<'_, Q, P>,
    ) -> Result<StreamOutcome, Error>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
    {
        let mut stats = HttpStats::default();
        let done = Stopwatch::new();
        let guard = self.acquire_processing()?;
        let idempotent = is_idempotent(&params.method);

        let req = match self.prepare_request(&mut stats, params).await {
            Ok(req) => req,
            Err(err) => {
                stats.total = done.elapsed_ms();
                run_on_done(&self.config, &stats, Some(&err)).await;
                return Err(err);
            }
        };

        let (res, guard) = match self
            .send_with_retry(&mut stats, idempotent, req, guard)
            .await
        {
            Ok(pair) => pair,
            Err(err) => {
                stats.total = done.elapsed_ms();
                run_on_done(&self.config, &stats, Some(&err)).await;
                return Err(err);
            }
        };

        let status = res.status().as_u16();
        if status >= 400 {
            let transfer_done = Stopwatch::new();
            let body_res = self.read_body_limited(res, &stats.path).await;
            stats.transfer = transfer_done.elapsed_ms();
            drop(guard);

            return match body_res {
                Ok(body) => {
                    stats.content_length = body.len();
                    if let Some(cb) = &self.config.circuit_breaker {
                        if status >= 500 {
                            cb.record_failure();
                        } else {
                            cb.record_success();
                        }
                    }
                    stats.total = done.elapsed_ms();
                    run_on_done(&self.config, &stats, None).await;
                    Ok(StreamOutcome::StatusError { status, body })
                }
                Err(err) => {
                    stats.total = done.elapsed_ms();
                    run_on_done(&self.config, &stats, Some(&err)).await;
                    Err(err)
                }
            };
        }

        if let Some(cb) = &self.config.circuit_breaker {
            cb.record_success();
        }
        stats.total = done.elapsed_ms();
        run_on_done(&self.config, &stats, None).await;

        let stream = GuardedByteStream {
            inner: Box::pin(res.bytes_stream()),
            _guard: guard,
            config: Arc::clone(&self.config),
            path: stats.path,
        };
        Ok(StreamOutcome::Stream(Box::pin(stream)))
    }
}

/// 表示 SSE 读取器的下一个事件结果。
#[derive(Debug)]
pub enum SsePoll {
    /// 拿到一行 `data:` 事件载荷（已去掉前缀与 CRLF，且保证 UTF-8 完整）。
    Data(String),
    /// 上游字节流自然耗尽。
    Done,
    /// 流传输或行长度超限错误。
    Err(Error),
}

/// 按字节缓冲的 SSE 读取器：按 `\n` 切出完整行后再做 UTF-8 解码，避免多字节字符跨 chunk 撕裂，
/// 同时同时支持标准 `data: <payload>` 与省略空格的 `data:<payload>`，并对单行长度施加上限。
pub struct SseReader {
    service: String,
    inner: BoxByteStream,
    buffer: BytesMut,
    max_line_bytes: usize,
}

impl fmt::Debug for SseReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SseReader")
            .field("service", &self.service)
            .field("buffered_bytes", &self.buffer.len())
            .field("max_line_bytes", &self.max_line_bytes)
            .finish()
    }
}

impl SseReader {
    /// 以服务名和底层字节流构建读取器，默认单行上限 [`DEFAULT_MAX_SSE_LINE_BYTES`]（16 MiB）。
    #[must_use]
    pub fn new(service: impl Into<String>, inner: BoxByteStream) -> Self {
        Self {
            service: service.into(),
            inner,
            buffer: BytesMut::new(),
            max_line_bytes: DEFAULT_MAX_SSE_LINE_BYTES,
        }
    }

    /// 覆盖单行最大字节数限制，超出则返回 `SsePoll::Err(Error::ResponseTooLarge)`。
    #[must_use]
    pub fn with_max_line_bytes(mut self, max_line_bytes: usize) -> Self {
        self.max_line_bytes = max_line_bytes;
        self
    }

    /// 拉取下一行非空的 `data:` 载荷；自动跳过空行、注释行（`:`）及 `event:`/`id:` 等非数据行。
    pub async fn next_data(&mut self) -> SsePoll {
        loop {
            if let Some(nl_pos) = self.buffer.iter().position(|&b| b == b'\n') {
                if nl_pos > self.max_line_bytes {
                    return SsePoll::Err(Error::ResponseTooLarge {
                        service: self.service.clone(),
                        limit: self.max_line_bytes,
                    });
                }
                let line_bytes = self.buffer.split_to(nl_pos + 1);
                let mut line = &line_bytes[..nl_pos];
                if let Some(stripped) = line.strip_suffix(b"\r") {
                    line = stripped;
                }
                let Ok(text) = std::str::from_utf8(line) else {
                    continue;
                };
                // WHATWG SSE 规范：冒号后允许零或一个可选 U+0020 空格，同时兼容 "data: x" 与 "data:x"
                let Some(data) = text
                    .strip_prefix("data: ")
                    .or_else(|| text.strip_prefix("data:"))
                else {
                    continue;
                };
                if data.is_empty() {
                    continue;
                }
                return SsePoll::Data(data.to_string());
            }

            if self.buffer.len() > self.max_line_bytes {
                return SsePoll::Err(Error::ResponseTooLarge {
                    service: self.service.clone(),
                    limit: self.max_line_bytes,
                });
            }

            match self.inner.next().await {
                Some(Ok(bytes)) => {
                    // 在把 chunk 写入缓冲区前就检验待拼接首行是否已超过上限，避免超大单块提前占满内存
                    let incoming_line_len = bytes
                        .iter()
                        .position(|&b| b == b'\n')
                        .unwrap_or(bytes.len());
                    if self.buffer.len().saturating_add(incoming_line_len) > self.max_line_bytes {
                        return SsePoll::Err(Error::ResponseTooLarge {
                            service: self.service.clone(),
                            limit: self.max_line_bytes,
                        });
                    }
                    self.buffer.extend_from_slice(&bytes);
                }
                Some(Err(err)) => return SsePoll::Err(err),
                None => return SsePoll::Done,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientBuilder;
    use axum::body::Body;
    use axum::routing::get;
    use futures::stream;
    use http::{Method, StatusCode};
    use pretty_assertions::assert_eq;

    fn reader_from_chunks(chunks: Vec<&[u8]>) -> SseReader {
        let items: Vec<Result<Bytes, Error>> = chunks
            .into_iter()
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect();
        SseReader::new("sse-test", Box::pin(stream::iter(items)))
    }

    async fn drain(reader: &mut SseReader) -> Vec<String> {
        let mut out = Vec::new();
        loop {
            match reader.next_data().await {
                SsePoll::Data(s) => out.push(s),
                SsePoll::Done => return out,
                SsePoll::Err(e) => panic!("unexpected error: {e}"),
            }
        }
    }

    #[tokio::test]
    async fn supports_data_with_and_without_space() {
        let mut reader = reader_from_chunks(vec![b"data: with_space\ndata:no_space\ndata:  two\n"]);
        assert_eq!(
            drain(&mut reader).await,
            vec![
                "with_space".to_string(),
                "no_space".to_string(),
                " two".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn utf8_multibyte_and_emoji_split_across_chunks() {
        let mut reader = reader_from_chunks(vec![
            b"data: \xE4\xBD",
            b"\xA0\xE5\xA5\xBD ",
            b"\xF0\x9F",
            b"\xA6\x80\n",
        ]);
        assert_eq!(drain(&mut reader).await, vec!["你好 🦀".to_string()]);
    }

    #[tokio::test]
    async fn line_exceeding_max_bytes_returns_response_too_large() {
        let mut reader = reader_from_chunks(vec![b"data: ", b"1234567890"]).with_max_line_bytes(10);
        match reader.next_data().await {
            SsePoll::Err(Error::ResponseTooLarge { limit, .. }) => assert_eq!(limit, 10),
            other => panic!("expected ResponseTooLarge, got {other:?}"),
        }

        // 换行前恰好超出上限也要拦截
        let mut exact_over = reader_from_chunks(vec![b"data: 123456\n"]).with_max_line_bytes(11);
        match exact_over.next_data().await {
            SsePoll::Err(Error::ResponseTooLarge { limit, .. }) => assert_eq!(limit, 11),
            other => panic!("expected ResponseTooLarge, got {other:?}"),
        }

        // 多行同块但每行均未超限，应正常放行
        let mut valid_packed = reader_from_chunks(vec![b"data:1\ndata:2\n"]).with_max_line_bytes(8);
        assert_eq!(
            drain(&mut valid_packed).await,
            vec!["1".to_string(), "2".to_string()]
        );
    }

    #[tokio::test]
    async fn execute_stream_holds_processing_guard_until_stream_drops_and_surfaces_status_error() {
        let app = axum::Router::new()
            .route(
                "/sse",
                get(|| async {
                    let s = stream::iter(vec![
                        Ok::<_, std::convert::Infallible>(Bytes::from_static(b"data: one\n\n")),
                        Ok(Bytes::from_static(b"data: two\n\n")),
                    ]);
                    Body::from_stream(s)
                }),
            )
            .route(
                "/err",
                get(|| async { (StatusCode::TOO_MANY_REQUESTS, r#"{"error":"rate limit"}"#) }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = ClientBuilder::new("sse-svc")
            .with_base_url(format!("http://{addr}"))
            .build()
            .unwrap();

        assert_eq!(client.get_processing(), 0);
        let outcome = client
            .execute_stream(Params::new(Method::GET, "/sse"))
            .await
            .unwrap();
        let StreamOutcome::Stream(stream) = outcome else {
            panic!("expected stream");
        };
        assert_eq!(client.get_processing(), 1);

        let mut reader = SseReader::new("sse-svc", stream);
        assert_eq!(
            drain(&mut reader).await,
            vec!["one".to_string(), "two".to_string()]
        );
        drop(reader);
        assert_eq!(client.get_processing(), 0);

        let err_outcome = client
            .execute_stream(Params::new(Method::GET, "/err"))
            .await
            .unwrap();
        match err_outcome {
            StreamOutcome::StatusError { status, body } => {
                assert_eq!(status, 429);
                assert_eq!(body.as_ref(), br#"{"error":"rate limit"}"#);
            }
            StreamOutcome::Stream(_) => panic!("expected StatusError"),
        }
        assert_eq!(client.get_processing(), 0);
    }
}
