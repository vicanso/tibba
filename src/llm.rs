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

//! LLM 流式对话演示 SSE 端点：`POST /llm/chat/stream`（需登录）。
//!
//! 从 `token_llms` 表读取名为 `default` 的启用配置（url / model / api_key / provider），
//! 用 [`tibba_llm::LlmCall`] 发起流式调用，把增量 token 通过 SSE（text/event-stream）
//! 逐段推给前端。仅在 `demo-token` feature 下编译（依赖可选的 tibba-model-token）。
//!
//! 事件约定：正常增量走默认（message）事件 `data: <delta>`；上游出错走 `event: error`；
//! 流末尾追加 `event: done`（`data: [DONE]`）方便前端 `EventSource` 收尾。

use crate::sql::get_db_pool;
use axum::Json;
use axum::Router;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::post;
use futures::StreamExt;
use serde::Deserialize;
use std::convert::Infallible;
use tibba_error::Error as BaseError;
use tibba_llm::{Backend, LlmCall};
use tibba_model_token::TokenLlmModel;
use tibba_session::UserSession;

type Result<T> = std::result::Result<T, BaseError>;

/// 本模块错误的统一分类，便于日志 / 前端按 category 归类。
const ERROR_CATEGORY: &str = "llm";
/// token_llms 表中约定的默认配置名。
const DEFAULT_LLM_NAME: &str = "default";
/// Anthropic provider 标识；其余 provider 一律走 OpenAI 兼容协议。
const PROVIDER_ANTHROPIC: &str = "anthropic";
/// 单条 prompt 长度上限，防止超大请求打爆上游。
const MAX_PROMPT_LEN: usize = 8_000;

/// 流式对话请求体。
#[derive(Debug, Deserialize)]
struct ChatStreamReq {
    /// 用户输入的 prompt。
    prompt: String,
}

/// `POST /llm/chat/stream` —— 登录用户发起一次流式对话，返回 SSE 事件流。
///
/// 错误处理沿用 bin 内约定：`tibba_model` / `tibba_llm` 的错误经 `?` 自动转
/// `BaseError`；请求参数 / 配置缺失这类域错误直接用 `BaseError::new(..)` 构造。
async fn chat_stream(
    _user: UserSession,
    Json(req): Json<ChatStreamReq>,
) -> Result<Sse<impl futures::Stream<Item = std::result::Result<Event, Infallible>>>> {
    let prompt = req.prompt.trim();
    if prompt.is_empty() {
        return Err(BaseError::new("prompt is empty")
            .with_category(ERROR_CATEGORY)
            .with_sub_category("invalid")
            .with_status(400));
    }
    if prompt.len() > MAX_PROMPT_LEN {
        return Err(
            BaseError::new(format!("prompt too long (max {MAX_PROMPT_LEN})"))
                .with_category(ERROR_CATEGORY)
                .with_sub_category("invalid")
                .with_status(400),
        );
    }

    // 读取默认 LLM 配置：tibba_model::Error 经 ? 自动转 BaseError
    let cfg = TokenLlmModel::default()
        .get_by_name(get_db_pool(), DEFAULT_LLM_NAME)
        .await?
        .ok_or_else(|| {
            // 未配置模型：503，让调用方知道是服务端未就绪而非请求错误
            BaseError::new("llm config `default` not found")
                .with_category(ERROR_CATEGORY)
                .with_sub_category("config_missing")
                .with_status(503)
        })?;

    // provider 映射到后端协议：anthropic 走 Claude，其余走 OpenAI 兼容格式
    let backend = if cfg.provider.eq_ignore_ascii_case(PROVIDER_ANTHROPIC) {
        Backend::Anthropic
    } else {
        Backend::OpenAi
    };

    let mut call = LlmCall::new(cfg.api_key, cfg.model, prompt).with_backend(backend);
    // 配置了自定义 base_url 才覆盖，否则用后端各自的官方默认地址
    if !cfg.url.is_empty() {
        call = call.with_base_url(cfg.url);
    }
    // 发起流式调用：tibba_llm::Error 经 ? 自动转 BaseError
    let chunks = call.chat_stream().await?;

    // StreamChunk 增量映射为 SSE 事件；末尾补 done 事件供前端收尾。
    // 上游中途出错时只回一个通用 error 事件，不外泄底层 URL / reqwest 细节。
    let events = chunks
        .map(|res| match res {
            Ok(chunk) => Event::default().data(chunk.delta),
            Err(_) => Event::default().event("error").data("stream error"),
        })
        .chain(futures::stream::once(async {
            Event::default().event("done").data("[DONE]")
        }))
        .map(Ok::<Event, Infallible>);

    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

/// 构造 LLM 演示路由（由 `router.rs` 以 `/llm` 前缀挂载）。端点均需登录。
pub fn new_llm_router() -> Router {
    Router::new().route("/chat/stream", post(chat_stream))
}
