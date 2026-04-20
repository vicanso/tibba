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

use super::{Error, LOG_TARGET};
use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::{Key, SignedCookieJar};
use cookie::CookieBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_state::CTX;
use tibba_util::{from_timestamp, timestamp, uuid};
use tracing::debug;

type Result<T, E = tibba_error::Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    SuperAdmin,
    Custom(String),
}

impl From<&str> for Role {
    fn from(s: &str) -> Self {
        match s {
            "admin" => Role::Admin,
            "su" => Role::SuperAdmin,
            _ => Role::Custom(s.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionParams {
    // secret for session cookie
    #[serde(skip)]
    key: Key,
    // cookie name
    cookie: String,
    // ttl of session
    ttl: i64,
    // max renewal count
    max_renewal: u8,
}

impl SessionParams {
    /// Creates a new SessionParams with the given signing key.
    pub fn new(key: Key) -> Self {
        Self {
            key,
            cookie: String::new(),
            ttl: 24 * 60 * 60,
            max_renewal: 0,
        }
    }

    /// Sets the cookie name used to store the session ID.
    #[must_use]
    pub fn with_cookie(mut self, cookie: impl Into<String>) -> Self {
        self.cookie = cookie.into();
        self
    }

    /// Sets the session TTL in seconds.
    #[must_use]
    pub fn with_ttl(mut self, ttl: i64) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the maximum number of session renewals allowed.
    #[must_use]
    pub fn with_max_renewal(mut self, max_renewal: u8) -> Self {
        self.max_renewal = max_renewal;
        self
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct SessionData {
    user_id: i64,
    // id
    id: String,
    // issued at
    iat: i64,
    // account
    account: String,
    // renewal count
    renewal_count: u8,
    // roles
    roles: Vec<String>,
    // groups
    groups: Vec<String>,
}

#[derive(Clone)]
pub struct Session {
    cache: &'static RedisCache,
    params: Arc<SessionParams>,
    data: SessionData,
}

impl Session {
    /// Creates a new session backed by the given cache and parameters.
    pub fn new(cache: &'static RedisCache, params: Arc<SessionParams>) -> Self {
        Self {
            cache,
            params,
            data: SessionData::default(),
        }
    }
    fn get_key(id: &str) -> String {
        format!("ss:{id}")
    }

    fn validate_login(&self) -> Result<()> {
        if !self.is_login() {
            return Err(Error::UserNotLogin.into());
        }
        Ok(())
    }

    /// Returns true if the session has an authenticated account.
    pub fn is_login(&self) -> bool {
        !self.data.account.is_empty()
    }

    /// Returns true if the session has not yet reached the renewal limit.
    pub fn can_renew(&self) -> bool {
        self.data.renewal_count < self.params.max_renewal
    }

    /// Sets the account and user ID, generating a new session ID when the account changes.
    #[must_use]
    pub fn with_account(mut self, account: impl Into<String>, user_id: i64) -> Self {
        let account = account.into();
        if self.data.id.is_empty() || self.data.account != account {
            self.data.id = uuid();
        }
        self.data.account = account;
        self.data.user_id = user_id;
        self.data.iat = timestamp();
        self
    }

    /// Sets the roles for this session.
    #[must_use]
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.data.roles = roles;
        self
    }

    /// Sets the groups for this session.
    #[must_use]
    pub fn with_groups(mut self, groups: Vec<String>) -> Self {
        self.data.groups = groups;
        self
    }

    /// Increments the renewal counter and updates the issued-at timestamp.
    pub fn refresh(&mut self) {
        self.data.renewal_count += 1;
        self.data.iat = timestamp();
    }

    /// Returns the authenticated account name.
    pub fn get_account(&self) -> &str {
        &self.data.account
    }

    /// Returns the authenticated user ID.
    pub fn get_user_id(&self) -> i64 {
        self.data.user_id
    }

    /// Returns the session expiry time as a formatted string.
    pub fn get_expired_at(&self) -> String {
        from_timestamp(self.data.iat + self.params.ttl, 0)
    }

    /// Returns true if the session will expire within the next hour.
    pub fn is_will_expired(&self) -> bool {
        self.data.iat + self.params.ttl - timestamp() < 3600
    }

    /// Returns the session issue time as a formatted string.
    pub fn get_issued_at(&self) -> String {
        from_timestamp(self.data.iat, 0)
    }

    /// Returns true if the session has passed its TTL.
    pub fn is_expired(&self) -> bool {
        self.data.iat + self.params.ttl < timestamp()
    }

    /// Clears the session ID and account, effectively logging out.
    pub fn reset(&mut self) {
        self.data.id = String::new();
        self.data.account = String::new();
    }

    /// Persists the session data to Redis.
    pub async fn save(&self) -> Result<()> {
        if self.data.id.is_empty() {
            return Err(Error::SessionIdEmpty.into());
        }
        self.cache
            .set_struct(
                &Self::get_key(&self.data.id),
                &self.data,
                Some(Duration::from_secs(self.params.ttl as u64)),
            )
            .await?;
        Ok(())
    }
}

impl TryFrom<&Session> for SignedCookieJar {
    type Error = tibba_error::Error;

    fn try_from(se: &Session) -> Result<Self, Self::Error> {
        let mut c = CookieBuilder::new(se.params.cookie.clone(), se.data.id.clone())
            .path("/")
            .http_only(true)
            .max_age(time::Duration::seconds(se.params.ttl));

        if se.data.id.is_empty() {
            c = c.max_age(time::Duration::days(0));
        }

        Ok(SignedCookieJar::new(se.params.key.clone()).add(c))
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SessionResp {
    account: String,
    renewal_count: u8,
}

impl IntoResponse for Session {
    fn into_response(self) -> Response {
        let result: Result<SignedCookieJar, _> = (&self).try_into();
        match result {
            Ok(jar) => (
                jar,
                Json(SessionResp {
                    account: self.data.account,
                    renewal_count: self.data.renewal_count,
                }),
            )
                .into_response(),
            Err(err) => err.into_response(),
        }
    }
}

pub struct SessionResponse<T>(pub Session, pub T);

impl<T> IntoResponse for SessionResponse<T>
where
    T: IntoResponse,
{
    fn into_response(self) -> Response {
        let result: Result<SignedCookieJar, _> = (&self.0).try_into();
        match result {
            Ok(jar) => (jar, self.1).into_response(),
            Err(err) => err.into_response(),
        }
    }
}

impl<S> FromRequestParts<S> for Session
where
    S: Send + Sync,
{
    type Rejection = tibba_error::Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let mut se = parts
            .extensions
            .get::<Session>()
            .ok_or::<Error>(Error::SessionNotFound)?
            .clone();
        debug!(
            target: LOG_TARGET,
            id = se.data.id,
            iat = se.data.iat,
            "from_request_parts"
        );
        // not fetch
        if se.data.iat == 0 {
            let jar = SignedCookieJar::from_headers(&parts.headers, se.params.key.clone());
            let Some(c) = jar.get(&se.params.cookie) else {
                return Ok(se);
            };
            let session_id = c.value();
            if session_id.len() < 36 {
                return Err(Error::SessionIdInvalid.into());
            }
            if let Some(data) = se
                .cache
                .get_struct::<SessionData>(&Session::get_key(session_id))
                .await?
            {
                debug!(
                    target: LOG_TARGET,
                    id = data.id,
                    iat = data.iat,
                    "load from cache"
                );
                se.data = data;
                parts.extensions.insert(se.clone());
                if se.is_login() {
                    CTX.get().set_account(se.get_account());
                }

                return Ok(se);
            }
        }
        Ok(se)
    }
}

pub struct UserSession(Session);

impl From<UserSession> for Session {
    fn from(se: UserSession) -> Self {
        se.0
    }
}

impl<S> FromRequestParts<S> for UserSession
where
    S: Send + Sync,
{
    type Rejection = tibba_error::Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let se = Session::from_request_parts(parts, _state).await?;
        se.validate_login()?;
        Ok(UserSession(se))
    }
}

impl std::ops::Deref for UserSession {
    type Target = Session;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for UserSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct AdminSession(Session);

impl From<AdminSession> for Session {
    fn from(se: AdminSession) -> Self {
        se.0
    }
}

impl<S> FromRequestParts<S> for AdminSession
where
    S: Send + Sync,
{
    type Rejection = tibba_error::Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let se = Session::from_request_parts(parts, _state).await?;
        se.validate_login()?;
        if !se.data.roles.iter().any(|role| {
            let r = Role::from(role.as_str());
            r == Role::Admin || r == Role::SuperAdmin
        }) {
            return Err(Error::UserNotAdmin.into());
        }
        Ok(AdminSession(se))
    }
}

impl std::ops::Deref for AdminSession {
    type Target = Session;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for AdminSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
