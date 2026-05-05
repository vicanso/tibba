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

//! Anthropic Messages API 客户端（Claude 系列模型）。

use super::{
    BoxStream, ChatParams, ChatResponse, Error, JsonSnafu, LOG_TARGET, RequestSnafu, Role,
    SseState, StreamChunk, Usage,
};
use futures::StreamExt;
use reqwest::Client as ReqwestClient;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::time::Duration;
use tracing::info;

type Result<T> = std::result::Result<T, Error>;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// Anthropic 要求的 API 版本头。
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Anthropic 要求的 beta 功能头（可选，保留用于未来扩展）。
const DEFAULT_MAX_TOKENS: u32 = 1024;

// ── 请求/响应 JSON 结构 ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct ReqMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct MessagesReq<'a> {
    model: &'a str,
    messages: Vec<ReqMessage<'a>>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    stream: bool,
}

#[derive(Deserialize)]
struct RespContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct RespUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct MessagesResp {
    content: Vec<RespContent>,
    model: String,
    usage: Option<RespUsage>,
}

/// Anthropic 错误响应体
#[derive(Deserialize)]
struct ErrBody {
    error: ErrDetail,
}

#[derive(Deserialize)]
struct ErrDetail {
    message: String,
}

// ── 流式事件结构（SSE data JSON）─────────────────────────────────────────────

/// `content_block_delta` 事件的 delta 字段。
#[derive(Deserialize)]
struct TextDelta {
    #[serde(rename = "type")]
    delta_type: String,
    text: Option<String>,
}

/// Anthropic SSE data 的通用结构，只解析我们关心的字段。
#[derive(Deserialize)]
struct SseData {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<TextDelta>,
}

// ── 流解析 ────────────────────────────────────────────────────────────────────

/// 将 Anthropic SSE 字节流解析为 StreamChunk 序列。
/// Anthropic SSE 格式：`event: <type>\ndata: <json>\n\n`
/// 仅提取 `content_block_delta` 中 `text_delta` 类型的内容。
fn parse_stream(response: reqwest::Response) -> BoxStream<Result<StreamChunk>> {
    let state = SseState::new(response);

    Box::pin(futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(nl) = state.buffer.find('\n') {
                let line = state.buffer[..nl].trim_end_matches('\r').to_string();
                state.buffer = state.buffer[nl + 1..].to_string();

                // 跳过 event: 行，只处理 data: 行
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data.is_empty() {
                    continue;
                }

                let sse: SseData = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(e) => return Some((Err(Error::Json { source: e }), state)),
                };

                match sse.event_type.as_str() {
                    "message_stop" => return None,
                    "content_block_delta" => {
                        if let Some(delta) = sse.delta
                            && delta.delta_type == "text_delta"
                        {
                            let chunk = StreamChunk {
                                delta: delta.text.unwrap_or_default(),
                                finish_reason: None,
                            };
                            return Some((Ok(chunk), state));
                        }
                    }
                    "message_delta" => {
                        // 流式结束，发一个带 finish_reason 的空 chunk
                        let chunk = StreamChunk {
                            delta: String::new(),
                            finish_reason: Some("stop".to_string()),
                        };
                        return Some((Ok(chunk), state));
                    }
                    _ => {}
                }
                continue;
            }

            // 缓冲区内没有完整行，继续读取字节流
            match state.inner.next().await {
                Some(Ok(bytes)) => {
                    state.buffer.push_str(&String::from_utf8_lossy(&bytes));
                }
                Some(Err(e)) => {
                    return Some((Err(Error::Request { source: e }), state));
                }
                None => return None,
            }
        }
    }))
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Anthropic 客户端配置。
struct AnthropicConfig {
    api_key: String,
    base_url: String,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    default_model: Option<String>,
}

/// Anthropic 客户端构建器。
pub struct AnthropicClientBuilder {
    config: AnthropicConfig,
}

impl AnthropicClientBuilder {
    /// 以 API Key 创建构建器。
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            config: AnthropicConfig {
                api_key: api_key.into(),
                base_url: DEFAULT_BASE_URL.to_string(),
                timeout: None,
                connect_timeout: None,
                default_model: None,
            },
        }
    }

    /// 覆盖 base URL，用于代理或私有部署场景。
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.config.base_url = base_url.into();
        self
    }

    /// 设置整体请求超时时间。
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = Some(timeout);
        self
    }

    /// 设置 TCP 连接超时时间。
    #[must_use]
    pub fn with_connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.config.connect_timeout = Some(connect_timeout);
        self
    }

    /// 设置当 `ChatParams.model` 为空时使用的默认模型。
    #[must_use]
    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.config.default_model = Some(model.into());
        self
    }

    /// 构建 `AnthropicClient`。
    pub fn build(self) -> Result<AnthropicClient> {
        let mut api_key_value =
            HeaderValue::from_str(&self.config.api_key).map_err(|e| Error::Stream {
                message: e.to_string(),
            })?;
        api_key_value.set_sensitive(true);

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", api_key_value);
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let mut builder = ReqwestClient::builder().default_headers(headers);
        if let Some(t) = self.config.timeout {
            builder = builder.timeout(t);
        }
        if let Some(t) = self.config.connect_timeout {
            builder = builder.connect_timeout(t);
        }

        let client = builder.build().context(RequestSnafu)?;

        Ok(AnthropicClient {
            client,
            base_url: self.config.base_url,
            default_model: self.config.default_model,
        })
    }
}

// ── 客户端 ────────────────────────────────────────────────────────────────────

/// Anthropic Messages API 客户端（Claude 系列）。
pub struct AnthropicClient {
    client: ReqwestClient,
    base_url: String,
    default_model: Option<String>,
}

impl AnthropicClient {
    fn resolve_model<'a>(&'a self, params: &'a ChatParams) -> &'a str {
        if !params.model.is_empty() {
            &params.model
        } else {
            self.default_model.as_deref().unwrap_or("claude-sonnet-4-6")
        }
    }

    /// 将消息列表拆分为 (system_prompt, non_system_messages)。
    /// Anthropic 的 messages 数组不允许 system role，system 需单独传字段。
    fn split_messages<'a>(&self, params: &'a ChatParams) -> (Option<&'a str>, Vec<ReqMessage<'a>>) {
        let mut system: Option<&str> = None;
        let mut messages = Vec::new();
        for m in &params.messages {
            match m.role {
                Role::System => system = Some(&m.content),
                Role::User => messages.push(ReqMessage {
                    role: "user",
                    content: &m.content,
                }),
                Role::Assistant => messages.push(ReqMessage {
                    role: "assistant",
                    content: &m.content,
                }),
            }
        }
        (system, messages)
    }

    /// 非流式调用，返回完整响应内容。
    pub async fn chat(&self, params: &ChatParams) -> Result<ChatResponse> {
        let model = self.resolve_model(params);
        let (system, messages) = self.split_messages(params);
        let req_body = MessagesReq {
            model,
            messages,
            max_tokens: params.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            system,
            temperature: params.temperature,
            top_p: params.top_p,
            stream: false,
        };

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .json(&req_body)
            .send()
            .await
            .context(RequestSnafu)?;

        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.context(RequestSnafu)?;

        if status >= 400 {
            let message = serde_json::from_slice::<ErrBody>(&bytes)
                .map(|b| b.error.message)
                .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
            return Err(Error::Api {
                service: "anthropic".to_string(),
                message,
            });
        }

        let body: MessagesResp = serde_json::from_slice(&bytes).context(JsonSnafu)?;

        let content = body
            .content
            .into_iter()
            .filter(|c| c.content_type == "text")
            .filter_map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");

        let usage = body.usage.map(|u| Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        });

        info!(
            target: LOG_TARGET,
            model,
            input_tokens = usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
            output_tokens = usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
            "anthropic chat completed",
        );

        Ok(ChatResponse {
            content,
            model: body.model,
            usage,
        })
    }

    /// 流式调用，返回增量 delta 片段流。
    pub async fn chat_stream(&self, params: &ChatParams) -> Result<BoxStream<Result<StreamChunk>>> {
        let model = self.resolve_model(params);
        let (system, messages) = self.split_messages(params);
        let req_body = MessagesReq {
            model,
            messages,
            max_tokens: params.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            system,
            temperature: params.temperature,
            top_p: params.top_p,
            stream: true,
        };

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .json(&req_body)
            .send()
            .await
            .context(RequestSnafu)?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let bytes = resp.bytes().await.context(RequestSnafu)?;
            let message = serde_json::from_slice::<ErrBody>(&bytes)
                .map(|b| b.error.message)
                .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
            return Err(Error::Api {
                service: "anthropic".to_string(),
                message,
            });
        }

        Ok(parse_stream(resp))
    }
}
