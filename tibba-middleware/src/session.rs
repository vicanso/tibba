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

use axum::extract::FromRequestParts;
use axum::extract::Request;
use axum::extract::State;
use axum::http::header::{HeaderMap, HeaderValue};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::cookie::{Key, SignedCookieJar};
use scopeguard::defer;
use serde::{Deserialize, Serialize};
use tibba_cache::RedisCache;
use tibba_error::Error;
use tibba_error::new_error;
use tibba_util::timestamp;
use tracing::debug;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Claim {
    // expiration time
    exp: u64,
    // issued at
    iat: u64,
    // id
    id: String,
    // account
    account: String,
}

impl Claim {
    pub fn with_account(mut self, account: String) -> Self {
        self.account = account;
        self
    }
}

impl<S> FromRequestParts<S> for Claim
where
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let claim = parts
            .extensions
            .get::<Claim>()
            .ok_or::<Error>(new_error("Claim not found").into())?;
        Ok(claim.clone())
    }
}

#[derive(Debug, Clone)]
pub struct SessionParams {
    pub prefixes: Vec<String>,
    pub secret: String,
    pub cookie: String,
    pub ttl_seconds: u64,
}

impl SessionParams {
    pub fn new(prefixes: Vec<String>) -> Self {
        Self {
            prefixes,
            secret: String::new(),
            cookie: String::new(),
            ttl_seconds: 2 * 24 * 3600,
        }
    }
    pub fn with_secret(mut self, secret: String) -> Self {
        self.secret = secret;
        self
    }
    pub fn with_cookie(mut self, cookie: String) -> Self {
        self.cookie = cookie;
        self
    }
    pub fn with_ttl_seconds(mut self, ttl_seconds: u64) -> Self {
        self.ttl_seconds = ttl_seconds;
        self
    }
}

async fn get_claim(
    headers: &HeaderMap<HeaderValue>,
    cache: &RedisCache,
    params: &SessionParams,
) -> Result<Claim> {
    let key = Key::try_from(params.secret.as_bytes()).map_err(|e| {
        new_error(&e.to_string())
            .with_category("session")
            .with_status(500)
            .with_exception(true)
    })?;
    let jar = SignedCookieJar::from_headers(headers, key);
    let Some(session_id) = jar.get(&params.cookie) else {
        return Ok(Claim::default());
    };
    let claim = cache.get_struct(session_id.value()).await?;
    Ok(claim.unwrap_or_default())
}

pub async fn session(
    State((cache, params)): State<(&'static RedisCache, &'static SessionParams)>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let path = req.uri().path();
    // for better performance, skip session for other paths
    if !params.prefixes.iter().any(|item| path.starts_with(item)) {
        let res = next.run(req).await;
        return Ok(res);
    }
    debug!(category = "middleware", "--> session");
    defer!(debug!(category = "middleware", "<-- session"););

    let mut claim = get_claim(req.headers(), cache, params).await?;
    if claim.iat == 0 {
        let iat = timestamp() as u64;
        claim.iat = iat;
        claim.exp = iat + params.ttl_seconds;
    }
    req.extensions_mut().insert(claim);
    let res = next.run(req).await;
    Ok(res)
}
