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

use super::Error;
use axum::Json;
use axum::extract::FromRequestParts;
use axum::extract::Request;
use axum::extract::State;
use axum::http::header::{HeaderMap, HeaderValue};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::{Key, SignedCookieJar};
use cookie::CookieBuilder;
use derivative::Derivative;
use scopeguard::defer;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_util::{from_timestamp, timestamp, uuid};
use tracing::debug;

type Result<T> = std::result::Result<T, tibba_error::Error>;

#[derive(Serialize, Deserialize, Default, Clone, Derivative)]
#[derivative(Debug)]
pub struct Claim {
    #[serde(skip)]
    #[derivative(Debug = "ignore")]
    cache: Option<&'static RedisCache>,
    #[serde(skip)]
    secret: String,
    cookie: String,
    // ttl in seconds
    ttl: i64,
    // id
    id: String,
    // issued at
    iat: i64,
    // account
    account: String,
    // renewal count
    renewal_count: u8,
}

impl Claim {
    fn get_key(id: &str) -> String {
        format!("ss:{id}")
    }
    pub fn can_renew(&self) -> bool {
        self.renewal_count < 10
    }
    pub fn with_account(mut self, account: String) -> Self {
        if self.id.is_empty() || self.account != account {
            self.id = uuid();
        }
        if self.account == account {
            self.renewal_count += 1;
        }
        self.account = account;
        self.iat = timestamp();
        self
    }
    pub fn get_account(&self) -> String {
        self.account.clone()
    }
    pub fn get_expired_at(&self) -> String {
        from_timestamp(self.iat + self.ttl, 0)
    }
    pub fn is_will_expired(&self) -> bool {
        self.iat + self.ttl - timestamp() < 3600
    }
    pub fn get_issued_at(&self) -> String {
        from_timestamp(self.iat, 0)
    }
    pub fn is_expired(&self) -> bool {
        self.iat + self.ttl < timestamp()
    }
    pub fn reset(&mut self) {
        self.id = "".to_string();
        self.account = "".to_string();
    }
    pub async fn save(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(Error::SessionIdEmpty.into());
        }
        let Some(cache) = self.cache else {
            return Err(Error::SessionCacheNotSet.into());
        };
        cache
            .set_struct(
                &Self::get_key(&self.id),
                &self,
                Some(Duration::from_secs(self.ttl as u64)),
            )
            .await?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ClaimResp {
    account: String,
    renewal_count: u8,
}

impl IntoResponse for Claim {
    fn into_response(self) -> Response {
        let c = CookieBuilder::new(self.cookie, self.id.clone())
            .path("/")
            .http_only(true)
            .max_age(time::Duration::seconds(self.ttl));

        match Key::try_from(self.secret.as_bytes()).map_err(|e| Error::Key { source: e }) {
            Ok(key) => {
                let jar = SignedCookieJar::new(key);
                (
                    jar.add(c),
                    Json(ClaimResp {
                        account: self.account,
                        renewal_count: self.renewal_count,
                    }),
                )
                    .into_response()
            }
            Err(e) => {
                let err: tibba_error::Error = e.into();
                err.into_response()
            }
        }
    }
}

impl<S> FromRequestParts<S> for Claim
where
    S: Send + Sync,
{
    type Rejection = tibba_error::Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let claim = parts
            .extensions
            .get::<Claim>()
            .ok_or::<Error>(Error::ClaimNotFound)?;
        Ok(claim.clone())
    }
}

#[derive(Debug, Clone)]
pub struct SessionParams {
    pub prefixes: Vec<String>,
    pub secret: String,
    pub cookie: String,
    pub ttl_seconds: i64,
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
    pub fn with_ttl_seconds(mut self, ttl_seconds: i64) -> Self {
        self.ttl_seconds = ttl_seconds;
        self
    }
}

async fn get_claim(
    headers: &HeaderMap<HeaderValue>,
    cache: &RedisCache,
    params: &SessionParams,
) -> Result<Claim> {
    let key = Key::try_from(params.secret.as_bytes()).map_err(|e| Error::Key { source: e })?;
    let jar = SignedCookieJar::from_headers(headers, key);
    let Some(session_id) = jar.get(&params.cookie) else {
        return Ok(Claim::default());
    };
    let claim = cache
        .get_struct(&Claim::get_key(session_id.value()))
        .await?;
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
    claim.ttl = params.ttl_seconds;
    claim.secret = params.secret.clone();
    claim.cookie = params.cookie.clone();
    claim.cache = Some(cache);
    if claim.iat == 0 {
        claim.iat = timestamp();
    }
    // reset if expired
    if claim.is_expired() {
        claim.reset();
    }
    req.extensions_mut().insert(claim);
    let res = next.run(req).await;
    Ok(res)
}
