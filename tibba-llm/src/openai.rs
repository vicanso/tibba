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

//! OpenAI 兼容格式客户端（也支持 DeepSeek、Qwen、Llama 等 OpenAI 兼容接口）。

use super::{
    BoxStream, ChatParams, ChatResponse, Error, JsonSnafu, LOG_TARGET, RequestSnafu, Role,
    SseState, StreamChunk, Usage,
};
use futures::StreamExt;
use reqwest::Client as ReqwestClient;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::time::Duration;
use tracing::info;

type Result<T> = std::result::Result<T, Error>;

const DEFAULT_BASE_URL: &str = "https://api.openai.com";

// ── 请求/响应 JSON 结构 ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct ReqMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatCompletionReq<'a> {
    model: &'a str,
    messages: Vec<ReqMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    stream: bool,
}

#[derive(Deserialize)]
struct RespUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct RespMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct RespChoice {
    message: RespMessage,
}

#[derive(Deserialize)]
struct ChatCompletionResp {
    choices: Vec<RespChoice>,
    model: String,
    usage: Option<RespUsage>,
}

/// 流式 delta 片段
#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct StreamResp {
    choices: Vec<StreamChoice>,
}

/// OpenAI 错误响应
#[derive(Deserialize)]
struct ErrBody {
    error: ErrDetail,
}

#[derive(Deserialize)]
struct ErrDetail {
    message: String,
}

// ── 流解析 ────────────────────────────────────────────────────────────────────

/// 将 SSE 字节流解析为 OpenAI StreamChunk 序列。
/// 格式：`data: <json>\n\n`，结束标志：`data: [DONE]\n\n`。
fn parse_stream(response: reqwest::Response) -> BoxStream<Result<StreamChunk>> {
    let state = SseState::new(response);

    Box::pin(futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(nl) = state.buffer.find('\n') {
                let line = state.buffer[..nl].trim_end_matches('\r').to_string();
                state.buffer = state.buffer[nl + 1..].to_string();

                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };

                if data == "[DONE]" {
                    return None;
                }
                if data.is_empty() {
                    continue;
                }

                let chunk = match serde_json::from_str::<StreamResp>(data) {
                    Ok(resp) => {
                        if let Some(choice) = resp.choices.into_iter().next() {
                            Ok(StreamChunk {
                                delta: choice.delta.content.unwrap_or_default(),
                                finish_reason: choice.finish_reason,
                            })
                        } else {
                            continue;
                        }
                    }
                    Err(e) => Err(Error::Json { source: e }),
                };
                return Some((chunk, state));
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

/// OpenAI 客户端配置。
struct OpenAiConfig {
    api_key: String,
    base_url: String,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    default_model: Option<String>,
}

/// OpenAI 客户端构建器。
pub struct OpenAiClientBuilder {
    config: OpenAiConfig,
}

impl OpenAiClientBuilder {
    /// 以 API Key 创建构建器。
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            config: OpenAiConfig {
                api_key: api_key.into(),
                base_url: DEFAULT_BASE_URL.to_string(),
                timeout: None,
                connect_timeout: None,
                default_model: None,
            },
        }
    }

    /// 覆盖 base URL，用于接入 OpenAI 兼容的第三方服务（如 DeepSeek、Qwen）。
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

    /// 构建 `OpenAiClient`。
    pub fn build(self) -> Result<OpenAiClient> {
        let mut auth_value = HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
            .map_err(|e| Error::Stream {
                message: e.to_string(),
            })?;
        auth_value.set_sensitive(true);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth_value);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let mut builder = ReqwestClient::builder().default_headers(headers);
        if let Some(t) = self.config.timeout {
            builder = builder.timeout(t);
        }
        if let Some(t) = self.config.connect_timeout {
            builder = builder.connect_timeout(t);
        }

        let client = builder.build().context(RequestSnafu)?;

        Ok(OpenAiClient {
            client,
            base_url: self.config.base_url,
            default_model: self.config.default_model,
        })
    }
}

// ── 客户端 ────────────────────────────────────────────────────────────────────

/// OpenAI 兼容格式 LLM 客户端。
pub struct OpenAiClient {
    client: ReqwestClient,
    base_url: String,
    default_model: Option<String>,
}

impl OpenAiClient {
    fn resolve_model<'a>(&'a self, params: &'a ChatParams) -> &'a str {
        if !params.model.is_empty() {
            &params.model
        } else {
            self.default_model.as_deref().unwrap_or("gpt-4o")
        }
    }

    fn build_messages<'a>(&self, params: &'a ChatParams) -> Vec<ReqMessage<'a>> {
        params
            .messages
            .iter()
            .map(|m| ReqMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: &m.content,
            })
            .collect()
    }

    /// 非流式调用，返回完整响应内容。
    pub async fn chat(&self, params: &ChatParams) -> Result<ChatResponse> {
        let model = self.resolve_model(params);
        let req_body = ChatCompletionReq {
            model,
            messages: self.build_messages(params),
            max_tokens: params.max_tokens,
            temperature: params.temperature,
            top_p: params.top_p,
            stream: false,
        };

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
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
                service: "openai".to_string(),
                message,
            });
        }

        let body: ChatCompletionResp = serde_json::from_slice(&bytes).context(JsonSnafu)?;

        let content = body
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default();

        let usage = body.usage.map(|u| Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        });

        info!(
            target: LOG_TARGET,
            model,
            input_tokens = usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
            output_tokens = usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
            "openai chat completed",
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
        let req_body = ChatCompletionReq {
            model,
            messages: self.build_messages(params),
            max_tokens: params.max_tokens,
            temperature: params.temperature,
            top_p: params.top_p,
            stream: true,
        };

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
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
                service: "openai".to_string(),
                message,
            });
        }

        Ok(parse_stream(resp))
    }
}
