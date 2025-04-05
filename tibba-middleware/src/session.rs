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

use axum::extract::Request;
use axum::extract::State;
use axum::middleware::Next;
use axum::response::Response;
use scopeguard::defer;
use tibba_cache::RedisCache;
use tibba_session::{Session, SessionParams};
use tracing::debug;

type Result<T, E = tibba_error::Error> = std::result::Result<T, E>;

pub async fn session(
    State((cache, params, skip_prefixes)): State<(
        &'static RedisCache,
        &'static SessionParams,
        Vec<String>,
    )>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let path = req.uri().path();
    // for better performance, skip session for other paths
    if skip_prefixes.iter().any(|item| path.starts_with(item)) {
        let res = next.run(req).await;
        return Ok(res);
    }
    debug!(category = "middleware", "--> session");
    defer!(debug!(category = "middleware", "<-- session"););

    let se = Session::new(cache, params.clone());
    req.extensions_mut().insert(se);
    let res = next.run(req).await;
    Ok(res)
}
