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

//! Idempotency-Key 中间件 —— 重复请求自动复用首次响应。
//!
//! ## 用法
//!
//! ```ignore
//! use tibba_middleware::{idempotency, IdempotencyState};
//! use axum::middleware::from_fn_with_state;
//!
//! // TokenService::recharge 这类敏感路由按需挂上
//! let state = IdempotencyState::new(get_redis_cache());
//! Router::new()
//!     .route("/tokens/recharge", post(recharge))
//!     .layer(from_fn_with_state(state, idempotency))
//! ```
//!
//! ## 协议（与 Stripe / OpenAI 一致）
//! 1. 客户端在 POST/PUT/PATCH/DELETE 请求里传 `Idempotency-Key: <uuid>` header
//!    缺失 → 不强制，直通
//! 2. scope：优先 `session.user_id`，否则 client IP；同 scope 同 key 视为同一请求
//! 3. 命中缓存 → 直接重建并返回首次响应（不进 handler）
//! 4. 未命中 → 执行 handler；2xx / 4xx 的响应缓存 24h，5xx 不缓存（让客户端 retry 触发处理）
//! 5. Response body 上限 1MB，超出返回 503
//!
//! ## 不做的事（可按需扩展）
//! - 请求 body hash 比对：同 key 但 payload 不同应当 422，本版本未实现
//! - per-route scope 命名空间：当前 key 只按 (user/ip, idempotency_key) 拼，
//!   跨路由复用 key 会撞缓存——业务保证 key 唯一即可

use crate::{Error, LOG_TARGET};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_session::Session;
use tracing::{debug, warn};

type Result<T, E = BaseError> = std::result::Result<T, E>;

/// 客户端约定的 idempotency header 名。
pub const IDEMPOTENCY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");
/// 服务端在重放响应里附带的提示头，便于客户端排障 / 监控感知重放比例。
pub const REPLAY_HEADER: HeaderName = HeaderName::from_static("x-idempotency-replay");
const REDIS_PREFIX: &str = "idempotency:";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const MAX_BODY_BYTES: usize = 1024 * 1024; // 1 MB
const MAX_KEY_LEN: usize = 128;

#[derive(Clone)]
pub struct IdempotencyState {
    cache: &'static RedisCache,
}

impl IdempotencyState {
    pub fn new(cache: &'static RedisCache) -> Self {
        Self { cache }
    }
}

/// 缓存的响应快照。仅保留 status + body —— headers 主体不缓存
/// （保留每次的 request_id / set-cookie 等动态字段，避免错配）
#[derive(Serialize, Deserialize)]
struct CachedResponse {
    status: u16,
    body: Vec<u8>,
}

/// 中间件入口。对非状态变更方法直通。
pub async fn idempotency(
    State(state): State<IdempotencyState>,
    req: Request,
    next: Next,
) -> Result<Response> {
    // 安全方法直通——没有写入副作用，不需要去重
    if !matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        return Ok(next.run(req).await);
    }

    // 取并校验 key；缺失 / 过长 → 不强制，直通
    let key = match extract_key(req.headers()) {
        Some(k) => k,
        None => return Ok(next.run(req).await),
    };

    let scope = extract_scope(&req);
    let cache_key = format!("{REDIS_PREFIX}{scope}:{key}");

    // 缓存命中 → 直接重建响应
    match state.cache.get_struct::<CachedResponse>(&cache_key).await {
        Ok(Some(cached)) => {
            debug!(target: LOG_TARGET, scope, key, "idempotency replay");
            return Ok(replay_response(cached));
        }
        Ok(None) => {}
        Err(e) => {
            // 缓存层挂掉就当无 key，让请求正常走 handler。失败仅日志，不污染响应
            warn!(target: LOG_TARGET, error = %e, "idempotency cache get failed (passthrough)");
        }
    }

    // 执行 handler
    let response = next.run(req).await;
    let status = response.status();

    // 5xx 不缓存——让客户端 retry 时进入真正处理路径
    if status.is_server_error() {
        return Ok(response);
    }

    // 缓存 2xx / 4xx；body 超过上限返回 503（避免半截响应）
    let (parts, body) = response.into_parts();
    let body_bytes = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            warn!(target: LOG_TARGET, error = %e, "idempotency body too large (not cached)");
            return Err(Error::IdempotencyBodyTooLarge {
                limit_bytes: MAX_BODY_BYTES,
            }
            .into());
        }
    };

    let cached = CachedResponse {
        status: parts.status.as_u16(),
        body: body_bytes.to_vec(),
    };
    if let Err(e) = state
        .cache
        .set_struct(
            &cache_key,
            &cached,
            Some(Duration::from_secs(CACHE_TTL_SECS)),
        )
        .await
    {
        warn!(target: LOG_TARGET, error = %e, "idempotency cache set failed (not cached)");
    }

    Ok(Response::from_parts(parts, Body::from(body_bytes)))
}

/// 取并清洗 idempotency-key header。空 / 过长 / 非 ASCII → None。
fn extract_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get(IDEMPOTENCY_HEADER.as_str())
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= MAX_KEY_LEN)
        .map(str::to_string)
}

/// 取 scope：优先 session.user_id，否则 X-Forwarded-For 第一个 IP，再否则 anon。
fn extract_scope(req: &Request) -> String {
    if let Some(session) = req.extensions().get::<Session>()
        && session.is_login()
    {
        return format!("u:{}", session.get_user_id());
    }
    if let Some(ip) = client_ip_from_headers(req.headers()) {
        return format!("ip:{ip}");
    }
    "anon".to_string()
}

fn client_ip_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("X-Real-Ip")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
}

/// 用缓存的 status + body 重建一个 Response，并附 `X-Idempotency-Replay: 1` 提示头
fn replay_response(cached: CachedResponse) -> Response {
    let status = StatusCode::from_u16(cached.status).unwrap_or(StatusCode::OK);
    let mut resp = Response::new(Body::from(cached.body));
    *resp.status_mut() = status;
    resp.headers_mut()
        .insert(REPLAY_HEADER, HeaderValue::from_static("1"));
    resp
}
