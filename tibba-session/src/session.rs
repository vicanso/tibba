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

static ROLE_ADMIN: &str = "admin";
static ROLE_SUPER_ADMIN: &str = "su";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionParams {
    // secret for session cookie
    pub secret: String,
    // cookie name
    pub cookie: String,
    // ttl of session
    pub ttl: i64,
    // max renewal count
    pub max_renewal: u8,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct SessionData {
    user_id: u64,
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
    /// Create a new session
    ///
    /// # Arguments
    /// * `cache` - Redis cache
    /// * `params` - Session parameters
    ///
    /// # Returns
    /// * `Session` - A new session
    pub fn new(cache: &'static RedisCache, params: Arc<SessionParams>) -> Self {
        Self {
            cache,
            params,
            data: SessionData::default(),
        }
    }
    /// Get the session key
    ///
    /// # Arguments
    /// * `id` - Session ID
    ///
    /// # Returns
    /// * `String` - Session key
    fn get_key(id: &str) -> String {
        format!("ss:{id}")
    }
    /// Validate the login
    ///
    /// # Returns
    /// * `Result<()>` - Result of the validation
    fn validate_login(&self) -> Result<()> {
        if !self.is_login() {
            return Err(Error::UserNotLogin.into());
        }
        Ok(())
    }
    /// Check if the session is logged in
    ///
    /// # Returns
    /// * `bool` - True if the session is logged in
    pub fn is_login(&self) -> bool {
        !self.data.account.is_empty()
    }
    /// Check if the session can be renewed
    ///
    /// # Returns
    /// * `bool` - True if the session can be renewed
    pub fn can_renew(&self) -> bool {
        self.data.renewal_count < self.params.max_renewal
    }
    /// Set the account and user ID for the session
    ///
    /// # Arguments
    /// * `account` - Account
    /// * `user_id` - User ID
    ///
    /// # Returns
    /// * `Session` - A new session
    pub fn with_account(mut self, account: &str, user_id: u64) -> Self {
        if self.data.id.is_empty() || self.data.account != account {
            self.data.id = uuid();
        }
        self.data.account = account.to_string();
        self.data.user_id = user_id;
        self.data.iat = timestamp();
        self
    }
    /// Set the roles for the session
    ///
    /// # Arguments
    /// * `roles` - Roles
    ///
    /// # Returns
    /// * `Session` - A new session
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.data.roles = roles;
        self
    }
    /// Set the groups for the session
    ///
    /// # Arguments
    /// * `groups` - Groups
    ///
    /// # Returns
    /// * `Session` - A new session
    pub fn with_groups(mut self, groups: Vec<String>) -> Self {
        self.data.groups = groups;
        self
    }
    /// Refresh the session
    ///
    /// # Returns
    /// * `Session` - A new session
    pub fn refresh(&mut self) {
        self.data.renewal_count += 1;
        self.data.iat = timestamp();
    }
    /// Get the account
    ///
    /// # Returns
    /// * `String` - Account
    pub fn get_account(&self) -> String {
        self.data.account.clone()
    }
    /// Get the user ID
    ///
    /// # Returns
    /// * `u64` - User ID
    pub fn get_user_id(&self) -> u64 {
        self.data.user_id
    }
    /// Get the expired at
    ///
    /// # Returns
    /// * `String` - Expired at
    pub fn get_expired_at(&self) -> String {
        from_timestamp(self.data.iat + self.params.ttl, 0)
    }
    /// Check if the session will expire in the next hour
    ///
    /// # Returns
    /// * `bool` - True if the session will expire in the next hour
    pub fn is_will_expired(&self) -> bool {
        self.data.iat + self.params.ttl - timestamp() < 3600
    }
    /// Get the issued at
    ///
    /// # Returns
    /// * `String` - Issued at
    pub fn get_issued_at(&self) -> String {
        from_timestamp(self.data.iat, 0)
    }
    /// Check if the session is expired
    ///
    /// # Returns
    /// * `bool` - True if the session is expired
    pub fn is_expired(&self) -> bool {
        self.data.iat + self.params.ttl < timestamp()
    }
    /// Reset the session
    ///
    /// # Returns
    /// * `Session` - A new session
    pub fn reset(&mut self) {
        self.data.id = "".to_string();
        self.data.account = "".to_string();
    }
    /// Save the session
    ///
    /// # Returns
    /// * `Result<()>` - Result of the save
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
        let key =
            Key::try_from(se.params.secret.as_bytes()).map_err(|e| Error::Key { source: e })?;

        let jar = SignedCookieJar::new(key);
        Ok(jar.add(c))
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
            id = se.data.id,
            iat = se.data.iat,
            category = "session",
            "from_request_parts"
        );
        // not fetch
        if se.data.iat == 0 {
            let key =
                Key::try_from(se.params.secret.as_bytes()).map_err(|e| Error::Key { source: e })?;
            let jar = SignedCookieJar::from_headers(&parts.headers, key);
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
                    id = data.id,
                    iat = data.iat,
                    category = "session",
                    "load from cache"
                );
                se.data = data;
                parts.extensions.insert(se.clone());
                if se.is_login() {
                    CTX.get().set_account(&se.get_account());
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
        Ok(UserSession(se.clone()))
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
        if !se
            .data
            .roles
            .iter()
            .any(|role| role == ROLE_ADMIN || role == ROLE_SUPER_ADMIN)
        {
            return Err(Error::UserNotAdmin.into());
        }
        Ok(AdminSession(se.clone()))
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
