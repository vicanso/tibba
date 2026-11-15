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
//!    - 缺失 → 不强制，直通
//!    - 存在但为空、含空白/控制字符或长度 > 128 → 返回 400
//! 2. scope：`user_id` / `account` / 脱敏认证指纹 / 客户端 IP，再拼接 HTTP Method 与 URI Path 哈希，
//!    防止同 key 跨用户或跨路由撞缓存
//! 3. 并发锁：缓存未命中时申请短 TTL 分布式锁（`lock_with_token`），并发重复请求返回 409 Conflict
//! 4. 未命中 → 执行 handler；2xx / 4xx（不含 5xx、不含 `Set-Cookie`）且体积 ≤ 1MB 的响应
//!    记录 `status` + `Content-Type` + `body` 缓存 24h
//! 5. Response body 上限 1MB，超出返回 503

use crate::{Error, LOG_TARGET, fnv1a_64};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_runtime::CTX;
use tibba_session::Session;
use tracing::{debug, warn};

type Result<T, E = BaseError> = std::result::Result<T, E>;

/// 客户端约定的 idempotency header 名。
pub const IDEMPOTENCY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");
/// 服务端在重放响应里附带的提示头，便于客户端排障 / 监控感知重放比例。
pub const REPLAY_HEADER: HeaderName = HeaderName::from_static("x-idempotency-replay");
const API_KEY_HEADER_NAME: &str = "x-api-key";
const REDIS_PREFIX: &str = "idempotency:";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const LOCK_TTL_SECS: u64 = 30;
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

/// 缓存的响应快照：保留 status、Content-Type 与 body，不缓存 request_id / set-cookie 等动态头。
#[derive(Serialize, Deserialize)]
struct CachedResponse {
    status: u16,
    #[serde(default)]
    content_type: Option<String>,
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

    // 缺失 Idempotency-Key 不强制；显式提供时严格校验格式
    let Some(key) = extract_key(req.headers())? else {
        return Ok(next.run(req).await);
    };

    let scope = extract_scope(&req);
    let cache_key = build_cache_key(&scope, req.method(), req.uri().path(), &key);
    let lock_key = format!("{cache_key}:lock");

    // 缓存命中 → 直接重建响应
    match state.cache.get_struct::<CachedResponse>(&cache_key).await {
        Ok(Some(cached)) => {
            debug!(target: LOG_TARGET, scope, "idempotency replay");
            return Ok(replay_response(cached));
        }
        Ok(None) => {}
        Err(e) => {
            // 缓存层挂掉就当无 key，让请求正常走 handler。失败仅日志，不污染响应
            warn!(target: LOG_TARGET, error = %e, "idempotency cache get failed (passthrough)");
        }
    }

    // 分布式互斥锁：防止并发重复请求同时击穿进入 handler
    let lock_token = match state
        .cache
        .lock_with_token(&lock_key, Some(Duration::from_secs(LOCK_TTL_SECS)))
        .await
    {
        Ok(Some(token)) => Some(token),
        Ok(None) => {
            // 抢锁失败：先复查一次是否刚写入完成；仍未完成则报 409 Conflict
            if let Ok(Some(cached)) = state.cache.get_struct::<CachedResponse>(&cache_key).await {
                debug!(target: LOG_TARGET, scope, "idempotency replay after lock contention");
                return Ok(replay_response(cached));
            }
            return Err(Error::IdempotencyConflict.into());
        }
        Err(e) => {
            warn!(target: LOG_TARGET, error = %e, "idempotency lock failed (passthrough)");
            None
        }
    };

    let response = next.run(req).await;
    let status = response.status();

    // 5xx 或携带 Set-Cookie 不缓存
    if status.is_server_error() || response.headers().contains_key(SET_COOKIE) {
        release_lock(state.cache, &lock_key, lock_token.as_ref()).await;
        return Ok(response);
    }

    // 缓存 2xx / 4xx；body 超过上限释放锁并返回 503
    let (parts, body) = response.into_parts();
    let body_bytes = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            release_lock(state.cache, &lock_key, lock_token.as_ref()).await;
            warn!(target: LOG_TARGET, error = %e, "idempotency body too large (not cached)");
            return Err(Error::IdempotencyBodyTooLarge {
                limit_bytes: MAX_BODY_BYTES,
            }
            .into());
        }
    };

    let content_type = parts
        .headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let cached = CachedResponse {
        status: parts.status.as_u16(),
        content_type,
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
    release_lock(state.cache, &lock_key, lock_token.as_ref()).await;

    Ok(Response::from_parts(parts, Body::from(body_bytes)))
}

async fn release_lock(
    cache: &'static RedisCache,
    lock_key: &str,
    token: Option<&tibba_cache::LockToken>,
) {
    if let Some(token) = token
        && let Err(e) = cache.unlock(lock_key, token).await
    {
        warn!(target: LOG_TARGET, error = %e, "idempotency unlock failed");
    }
}

/// 组装隔离良好的 Redis 键：`idempotency:<scope>:<method>:<path_hash>:<key_hash>`。
fn build_cache_key(scope: &str, method: &Method, path: &str, key: &str) -> String {
    format!(
        "{REDIS_PREFIX}{scope}:{}:{:016x}:{:016x}",
        method.as_str(),
        fnv1a_64(path.as_bytes()),
        fnv1a_64(key.as_bytes()),
    )
}

/// 取并校验 `Idempotency-Key` header：
/// - 缺失 → `Ok(None)`
/// - 存在但为空、超过 128 字节或含控制符/空白 → `Err(400)`
fn extract_key(headers: &HeaderMap) -> Result<Option<String>> {
    let Some(raw) = headers.get(IDEMPOTENCY_HEADER.as_str()) else {
        return Ok(None);
    };
    let s = raw
        .to_str()
        .map(str::trim)
        .map_err(|_| BaseError::from(Error::IdempotencyInvalidKey))?;
    if s.is_empty()
        || s.len() > MAX_KEY_LEN
        || s.bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return Err(Error::IdempotencyInvalidKey.into());
    }
    Ok(Some(s.to_string()))
}

/// 取调用方作用域：Session user_id → CTX account → 显式凭据/Cookie 哈希指纹 → X-Forwarded-For/X-Real-Ip → anon。
fn extract_scope(req: &Request) -> String {
    if let Some(session) = req.extensions().get::<Session>()
        && session.is_login()
    {
        return format!("u:{}", session.get_user_id());
    }
    if let Ok(acc) = CTX.try_with(|ctx| ctx.get_account())
        && !acc.is_empty()
    {
        return format!("a:{acc}");
    }
    for header in [AUTHORIZATION.as_str(), API_KEY_HEADER_NAME, COOKIE.as_str()] {
        if let Some(val) = req.headers().get(header).and_then(|v| v.to_str().ok())
            && !val.trim().is_empty()
        {
            return format!("c:{:016x}", fnv1a_64(val.as_bytes()));
        }
    }
    if let Some(ip) = client_ip_from_headers(req.headers()) {
        return format!("ip:{:016x}", fnv1a_64(ip.as_bytes()));
    }
    "anon".to_string()
}

fn client_ip_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("X-Real-Ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

/// 用缓存的 status + Content-Type + body 重建响应，并附 `X-Idempotency-Replay: 1` 提示头。
fn replay_response(cached: CachedResponse) -> Response {
    let status = StatusCode::from_u16(cached.status).unwrap_or(StatusCode::OK);
    let mut resp = Response::new(Body::from(cached.body));
    *resp.status_mut() = status;
    if let Some(ct) = cached.content_type
        && let Ok(val) = HeaderValue::from_str(&ct)
    {
        resp.headers_mut().insert(CONTENT_TYPE, val);
    }
    resp.headers_mut()
        .insert(REPLAY_HEADER, HeaderValue::from_static("1"));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn extract_key_accepts_missing_and_rejects_malformed() {
        let empty = HeaderMap::new();
        assert_eq!(extract_key(&empty).expect("missing is ok"), None);

        for bad in ["", "   ", "bad key", "bad\nkey", &"k".repeat(129)] {
            let mut h = HeaderMap::new();
            if let Ok(val) = HeaderValue::from_str(bad) {
                h.insert(IDEMPOTENCY_HEADER, val);
                let err = extract_key(&h).expect_err("malformed key must fail");
                assert_eq!(err.status(), 400);
                assert_eq!(err.sub_category(), Some("idempotency_invalid_key"));
            }
        }

        let mut ok = HeaderMap::new();
        ok.insert(IDEMPOTENCY_HEADER, HeaderValue::from_static("req-123-abc"));
        assert_eq!(
            extract_key(&ok).expect("valid key"),
            Some("req-123-abc".to_string())
        );
    }

    #[test]
    fn scope_hashes_credentials_and_key_isolates_method_and_path() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/pay")
            .header(AUTHORIZATION, "Bearer super_secret_tok")
            .body(Body::empty())
            .expect("req");
        let scope = extract_scope(&req);
        assert!(scope.starts_with("c:"), "{scope}");
        assert!(!scope.contains("super_secret_tok"));

        let k1 = build_cache_key(&scope, &Method::POST, "/v1/pay", "same-key");
        let k2 = build_cache_key(&scope, &Method::PUT, "/v1/pay", "same-key");
        let k3 = build_cache_key(&scope, &Method::POST, "/v1/refund", "same-key");
        assert_ne!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn replay_preserves_content_type_status_and_replay_header() {
        let resp = replay_response(CachedResponse {
            status: 201,
            content_type: Some("application/json".to_string()),
            body: b"{\"id\":1}".to_vec(),
        });
        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(
            resp.headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            resp.headers()
                .get(REPLAY_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some("1")
        );
    }
}
