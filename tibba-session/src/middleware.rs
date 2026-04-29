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

use super::{LOG_TARGET, Session, SessionParams};
use axum::extract::Request;
use axum::extract::State;
use axum::middleware::Next;
use axum::response::Response;
use scopeguard::defer;
use std::sync::Arc;
use tibba_cache::RedisCache;
use tracing::debug;

type Result<T, E = tibba_error::Error> = std::result::Result<T, E>;

/// axum 中间件：在请求扩展中注入空 Session 实例，供后续 handler 通过 extractor 按需加载。
/// Session 数据不在此处预加载，而是在 `FromRequestParts` 实现中按需从 Redis 读取。
pub async fn session(
    State((cache, params)): State<(&'static RedisCache, Arc<SessionParams>)>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    debug!(target: LOG_TARGET, "--> session");
    defer!(debug!(target: LOG_TARGET, "<-- session"););

    req.extensions_mut().insert(Session::new(cache, params));
    Ok(next.run(req).await)
}
