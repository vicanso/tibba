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
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_util::{from_timestamp, timestamp, uuid};
use tracing::debug;

type Result<T, E = tibba_error::Error> = std::result::Result<T, E>;

static ROLE_ADMIN: &str = "admin";
static ROLE_SUPER_ADMIN: &str = "su";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionParams {
    pub secret: String,
    pub cookie: String,
    pub ttl: i64,
    pub max_renewal: u8,
}

#[derive(Serialize, Deserialize, Default, Clone, Derivative)]
#[derivative(Debug)]
pub struct Session {
    #[serde(skip)]
    #[derivative(Debug = "ignore")]
    cache: Option<&'static RedisCache>,
    #[serde(skip)]
    params: SessionParams,
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

impl Session {
    pub fn new(cache: &'static RedisCache, params: SessionParams) -> Self {
        Self {
            cache: Some(cache),
            params,
            ..Default::default()
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
    pub fn is_login(&self) -> bool {
        !self.account.is_empty()
    }
    pub fn can_renew(&self) -> bool {
        self.renewal_count < self.params.max_renewal
    }
    pub fn with_account(mut self, account: &str) -> Self {
        if self.id.is_empty() || self.account != account {
            self.id = uuid();
        }
        self.account = account.to_string();
        self.iat = timestamp();
        self
    }
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }
    pub fn with_groups(mut self, groups: Vec<String>) -> Self {
        self.groups = groups;
        self
    }
    pub fn refresh(&mut self) {
        self.renewal_count += 1;
        self.iat = timestamp();
    }
    pub fn get_account(&self) -> String {
        self.account.clone()
    }
    pub fn get_expired_at(&self) -> String {
        from_timestamp(self.iat + self.params.ttl, 0)
    }
    pub fn is_will_expired(&self) -> bool {
        self.iat + self.params.ttl - timestamp() < 3600
    }
    pub fn get_issued_at(&self) -> String {
        from_timestamp(self.iat, 0)
    }
    pub fn is_expired(&self) -> bool {
        self.iat + self.params.ttl < timestamp()
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
                Some(Duration::from_secs(self.params.ttl as u64)),
            )
            .await?;
        Ok(())
    }
}

impl TryFrom<&Session> for SignedCookieJar {
    type Error = tibba_error::Error;

    fn try_from(se: &Session) -> Result<Self, Self::Error> {
        let mut c = CookieBuilder::new(se.params.cookie.clone(), se.id.clone())
            .path("/")
            .http_only(true)
            .max_age(time::Duration::seconds(se.params.ttl));

        if se.id.is_empty() {
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
                    account: self.account,
                    renewal_count: self.renewal_count,
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
        let se = parts
            .extensions
            .get::<Session>()
            .ok_or::<Error>(Error::SessionNotFound)?;
        debug!(
            id = se.id,
            iat = se.iat,
            category = "session",
            "from_request_parts"
        );
        let Some(cache) = se.cache else {
            return Err(Error::SessionCacheNotSet.into());
        };
        // not fetch
        if se.iat == 0 {
            let key =
                Key::try_from(se.params.secret.as_bytes()).map_err(|e| Error::Key { source: e })?;
            let jar = SignedCookieJar::from_headers(&parts.headers, key);
            let Some(session_id) = jar.get(&se.params.cookie) else {
                return Ok(Session {
                    iat: timestamp(),
                    ..se.clone()
                });
            };
            if let Some(mut data) = cache
                .get_struct::<Session>(&Session::get_key(session_id.value()))
                .await?
            {
                data.params = se.params.clone();
                data.cache = Some(cache);
                parts.extensions.insert(data.clone());
                debug!(
                    id = data.id,
                    iat = data.iat,
                    category = "session",
                    "load from cache"
                );
                return Ok(data);
            }
        }
        Ok(Session {
            iat: timestamp(),
            ..se.clone()
        })
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
