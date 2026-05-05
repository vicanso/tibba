// Copyright 2025 Tree xie.
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

use futures::Stream;
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use std::pin::Pin;
use tibba_error::Error as BaseError;

/// 本模块所有日志的 tracing target，可通过 `RUST_LOG=tibba:llm=debug` 过滤。
pub(crate) const LOG_TARGET: &str = "tibba:llm";

#[allow(dead_code)]
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
    /// SSE 流解析错误
    #[snafu(display("stream error: {message}"))]
    Stream { message: String },
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
            Error::Stream { message } => BaseError::new(message),
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
pub(crate) struct SseState {
    pub inner: Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    pub buffer: String,
}

impl SseState {
    pub(crate) fn new(response: reqwest::Response) -> Self {
        Self {
            inner: Box::pin(response.bytes_stream()),
            buffer: String::new(),
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
/// ```rust
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
