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

use bytes::BytesMut;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use std::pin::Pin;
use tibba_error::Error as BaseError;

/// 本模块所有日志的 tracing target，可通过 `RUST_LOG=tibba:llm=debug` 过滤。
pub(crate) const LOG_TARGET: &str = "tibba:llm";

/// 流式输出的装箱 Stream 类型别名。
pub type BoxStream<T> = Pin<Box<dyn Stream<Item = T> + Send>>;

// ── 错误类型 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    #[snafu(display("http request failed: {source}"))]
    Request { source: reqwest::Error },
    #[snafu(display("json error: {source}"))]
    Json { source: serde_json::Error },
    /// API 返回的业务错误（status ≥ 400 或 error 字段）
    #[snafu(display("{service} api error: {message}"))]
    Api { service: String, message: String },
    /// 构造 HTTP header 时失败（例如 API key 含非法字节）
    #[snafu(display("invalid header: {source}"))]
    InvalidHeader {
        source: reqwest::header::InvalidHeaderValue,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Request { source } => BaseError::new(source),
            Error::Json { source } => BaseError::new(source),
            Error::Api {
                service: _,
                message,
            } => BaseError::new(message),
            Error::InvalidHeader { source } => BaseError::new(source),
        };
        err.with_category("llm")
    }
}

// ── 公共消息类型 ──────────────────────────────────────────────────────────────

/// 消息角色。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// 单条对话消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// 统一的 LLM 请求参数，两种后端均接受此结构。
#[derive(Debug, Clone, Default)]
pub struct ChatParams {
    /// 模型 ID，例如 `"gpt-4o"` 或 `"claude-opus-4-7"`
    pub model: String,
    pub messages: Vec<Message>,
    /// 最大输出 token 数；Anthropic 接口此字段必填，默认 1024
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

impl ChatParams {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    #[must_use]
    pub fn add_message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    #[must_use]
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    #[must_use]
    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }
}

/// token 用量统计。
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// 非流式调用的完整响应。
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<Usage>,
}

/// 流式输出的单个增量片段。
#[derive(Debug, Clone, Default)]
pub struct StreamChunk {
    pub delta: String,
    pub finish_reason: Option<String>,
}

// ── 内部 SSE 解析工具 ─────────────────────────────────────────────────────────

/// 持有 SSE 响应体字节流和已收到但未解析的缓冲区。
///
/// 使用 `BytesMut` 而非 `String` 作为 buffer 的关键原因：上游 chunk 边界
/// 任意，可能切在 UTF-8 多字节字符中间。若每次 chunk 都 `from_utf8_lossy`
/// 一次，前半截字节会被替换成 U+FFFD，再接上后半截就成乱码——中文 / emoji
/// 等多字节内容的流式输出会被这个 bug 静默吃掉。改为按字节缓冲、提取出完整
/// 行后再做 UTF-8 解码，分界点永远落在 '\n' 上（ASCII），不再切到多字节中间。
pub(crate) struct SseState {
    pub inner: BoxStream<reqwest::Result<bytes::Bytes>>,
    pub buffer: BytesMut,
}

impl SseState {
    pub(crate) fn new(response: reqwest::Response) -> Self {
        Self {
            inner: Box::pin(response.bytes_stream()),
            buffer: BytesMut::new(),
        }
    }
}

/// 表示 SSE 解析的下一个事件结果。
pub(crate) enum SsePoll {
    /// 拿到一行 `data: <body>` 的 body（已去掉前缀和 CRLF）。
    Data(String),
    /// 流自然结束（上游字节流耗尽）。
    Done,
    /// 上游网络/IO 错误。
    Err(Error),
}

/// 从 SSE state 中拉取下一行 `data: ...` 的 body，自动跳过 `event:` / 空行 /
/// 非 `data:` 行；按字节查找 `\n` 后再解码 UTF-8，避免多字节字符在 chunk
/// 边界被截断（参见 [`SseState`] 上的设计说明）。
///
/// 之前 openai / anthropic 各有一份近 30 行的、几乎一致的解析循环，现在统一
/// 走这里——任何 SSE 协议细节修复只改一处。
pub(crate) async fn next_sse_data_line(state: &mut SseState) -> SsePoll {
    loop {
        // 尝试从已缓存字节中切出一整行
        if let Some(nl_pos) = state.buffer.iter().position(|&b| b == b'\n') {
            // split_to 是 O(1) 的指针调整，不会复制残余字节（原代码 String::to_string 是 O(n)）
            let line_bytes = state.buffer.split_to(nl_pos + 1);
            // 去掉行尾 \n 以及可能的 \r
            let mut line = &line_bytes[..nl_pos];
            if let Some(stripped) = line.strip_suffix(b"\r") {
                line = stripped;
            }
            // SSE 协议要求 UTF-8。整行字节是完整的，解码不会切到字符中间；
            // 极端情况（上游真的吐了非法 UTF-8 行）按 SSE 注释行处理跳过
            let Ok(text) = std::str::from_utf8(line) else {
                continue;
            };
            let Some(data) = text.strip_prefix("data: ") else {
                // event: / id: / : 注释 / 空行 等都不是 data，直接忽略
                continue;
            };
            if data.is_empty() {
                continue;
            }
            return SsePoll::Data(data.to_string());
        }

        // 缓冲区没有完整行，拉更多字节
        match state.inner.next().await {
            Some(Ok(bytes)) => state.buffer.extend_from_slice(&bytes),
            Some(Err(e)) => return SsePoll::Err(Error::Request { source: e }),
            None => return SsePoll::Done,
        }
    }
}

// ── 通用一次性调用封装 ────────────────────────────────────────────────────────

/// LLM 后端协议选择。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Backend {
    /// OpenAI 兼容格式（默认），也适用于 DeepSeek、Qwen 等第三方服务。
    #[default]
    OpenAi,
    /// Anthropic Messages API（Claude 系列）。
    Anthropic,
}

/// 一次性 LLM 调用的参数，内部自动构建客户端并执行请求。
///
/// ```ignore
/// use tibba_llm::{Backend, LlmCall};
///
/// // OpenAI 兼容接口
/// let resp = LlmCall::new(api_key, "gpt-4o-mini", "What is Rust?")
///     .with_system_message("You are a concise assistant.")
///     .chat()
///     .await?;
///
/// // Anthropic（Claude）接口
/// let resp = LlmCall::new(api_key, "claude-sonnet-4-6", "What is Rust?")
///     .with_backend(Backend::Anthropic)
///     .with_system_message("You are a concise assistant.")
///     .chat()
///     .await?;
/// ```
pub struct LlmCall {
    api_key: String,
    model: String,
    user_message: String,
    base_url: String,
    system_message: String,
    backend: Backend,
}

impl LlmCall {
    /// 创建调用，`api_key`、`model`、`user_message` 为必填，默认使用 OpenAI 协议。
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        user_message: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            user_message: user_message.into(),
            base_url: String::new(),
            system_message: String::new(),
            backend: Backend::default(),
        }
    }

    /// 选择后端协议，默认为 `Backend::OpenAi`。
    #[must_use]
    pub fn with_backend(mut self, backend: Backend) -> Self {
        self.backend = backend;
        self
    }

    /// 覆盖 API base URL；省略时各后端使用各自的官方默认地址。
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// 设置系统提示词，留空则不注入 system 消息。
    #[must_use]
    pub fn with_system_message(mut self, system_message: impl Into<String>) -> Self {
        self.system_message = system_message.into();
        self
    }

    fn build_params(&self) -> ChatParams {
        let mut params = ChatParams::new(self.model.clone());
        if !self.system_message.is_empty() {
            params = params.add_message(Message::system(self.system_message.clone()));
        }
        params.add_message(Message::user(self.user_message.clone()))
    }

    /// 非流式调用，返回完整 `ChatResponse`。
    pub async fn chat(self) -> Result<ChatResponse, Error> {
        let params = self.build_params();
        match self.backend {
            Backend::OpenAi => {
                let mut builder = openai::OpenAiClientBuilder::new(self.api_key);
                if !self.base_url.is_empty() {
                    builder = builder.with_base_url(self.base_url);
                }
                builder.build()?.chat(&params).await
            }
            Backend::Anthropic => {
                let mut builder = anthropic::AnthropicClientBuilder::new(self.api_key);
                if !self.base_url.is_empty() {
                    builder = builder.with_base_url(self.base_url);
                }
                builder.build()?.chat(&params).await
            }
        }
    }

    /// 流式调用，返回增量 `StreamChunk` 流。
    pub async fn chat_stream(self) -> Result<BoxStream<Result<StreamChunk, Error>>, Error> {
        let params = self.build_params();
        match self.backend {
            Backend::OpenAi => {
                let mut builder = openai::OpenAiClientBuilder::new(self.api_key);
                if !self.base_url.is_empty() {
                    builder = builder.with_base_url(self.base_url);
                }
                builder.build()?.chat_stream(&params).await
            }
            Backend::Anthropic => {
                let mut builder = anthropic::AnthropicClientBuilder::new(self.api_key);
                if !self.base_url.is_empty() {
                    builder = builder.with_base_url(self.base_url);
                }
                builder.build()?.chat_stream(&params).await
            }
        }
    }
}

// ── 子模块 ────────────────────────────────────────────────────────────────────

mod anthropic;
mod openai;

pub use anthropic::AnthropicClient;
pub use anthropic::AnthropicClientBuilder;
pub use openai::OpenAiClient;
pub use openai::OpenAiClientBuilder;

#[cfg(test)]
mod tests {
    //! SSE 解析器的单元测试。
    //! 通过构造内存字节流绕开真实 HTTP，专注验证 `next_sse_data_line`
    //! 对 chunk 边界、UTF-8 多字节、CRLF、注释行等的处理。

    use super::*;
    use bytes::Bytes;
    use futures::stream;
    use pretty_assertions::assert_eq;

    /// 用预先准备好的 chunk 列表构造一个 SseState，跳过真实 HTTP。
    fn state_from_chunks(chunks: Vec<&[u8]>) -> SseState {
        let owned: Vec<reqwest::Result<Bytes>> = chunks
            .into_iter()
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect();
        SseState {
            inner: Box::pin(stream::iter(owned)),
            buffer: BytesMut::new(),
        }
    }

    /// 依次拉所有 data 行，直到 Done。Err 时立即返回。
    async fn drain(state: &mut SseState) -> Vec<String> {
        let mut out = Vec::new();
        loop {
            match next_sse_data_line(state).await {
                SsePoll::Data(s) => out.push(s),
                SsePoll::Done => return out,
                SsePoll::Err(e) => panic!("unexpected error: {e}"),
            }
        }
    }

    #[tokio::test]
    async fn single_chunk_single_line() {
        let mut s = state_from_chunks(vec![b"data: hello\n\n"]);
        assert_eq!(drain(&mut s).await, vec!["hello".to_string()]);
    }

    #[tokio::test]
    async fn crlf_line_endings_are_stripped() {
        // Anthropic/某些代理可能用 \r\n
        let mut s = state_from_chunks(vec![b"data: first\r\ndata: second\r\n"]);
        assert_eq!(
            drain(&mut s).await,
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[tokio::test]
    async fn non_data_lines_are_skipped() {
        // event: / id: / : 注释 / 空行都应跳过，只剩 data 行
        let mut s = state_from_chunks(vec![
            b": this is a comment\n",
            b"event: message\n",
            b"id: 42\n",
            b"\n",
            b"data: payload\n",
        ]);
        assert_eq!(drain(&mut s).await, vec!["payload".to_string()]);
    }

    #[tokio::test]
    async fn line_split_across_chunks() {
        // 关键场景：一行被拆到两个 chunk
        let mut s = state_from_chunks(vec![b"data: hel", b"lo world\n"]);
        assert_eq!(drain(&mut s).await, vec!["hello world".to_string()]);
    }

    #[tokio::test]
    async fn utf8_multibyte_split_across_chunks_no_corruption() {
        // 「你好」= E4 BD A0 E5 A5 BD（6 字节）。在第 2 字节切开 chunk
        // 之前的 String::from_utf8_lossy 实现会把前 2 字节替换为 U+FFFD，
        // 后 4 字节继续 lossy 又一次替换 → 输出乱码
        // 现在按字节缓冲、解码整行：必须得到完整「你好」
        let mut s = state_from_chunks(vec![
            b"data: \xE4\xBD",     // "你" 的前 2 字节
            b"\xA0\xE5\xA5\xBD\n", // "你" 的尾字节 + "好" 全部
        ]);
        assert_eq!(drain(&mut s).await, vec!["你好".to_string()]);
    }

    #[tokio::test]
    async fn emoji_split_across_chunks_no_corruption() {
        // 🦀（U+1F980）= F0 9F A6 80（4 字节），在中间任意位置切都不能丢
        let mut s = state_from_chunks(vec![b"data: hi ", b"\xF0\x9F", b"\xA6\x80\n"]);
        assert_eq!(drain(&mut s).await, vec!["hi 🦀".to_string()]);
    }

    #[tokio::test]
    async fn empty_data_line_is_skipped() {
        let mut s = state_from_chunks(vec![b"data: \ndata: ok\n"]);
        assert_eq!(drain(&mut s).await, vec!["ok".to_string()]);
    }

    #[tokio::test]
    async fn stream_done_when_inner_exhausted_mid_line() {
        // 最后一行没有 \n —— 现实中合规 SSE 总有终止 \n，
        // 但我们的解析器对此应当不死锁，直接返回 Done
        let mut s = state_from_chunks(vec![b"data: incomplete"]);
        assert_eq!(drain(&mut s).await, Vec::<String>::new());
    }

    #[tokio::test]
    async fn many_chunks_one_byte_each() {
        // 病态：每个 chunk 只有 1 字节
        let bytes = b"data: abc\n".to_vec();
        let chunks: Vec<&[u8]> = bytes.iter().map(std::slice::from_ref).collect();
        let mut s = state_from_chunks(chunks);
        assert_eq!(drain(&mut s).await, vec!["abc".to_string()]);
    }
}
