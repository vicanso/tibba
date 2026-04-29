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

/// 用户角色枚举，支持内置角色（Admin / SuperAdmin）和自定义角色。
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

/// Session 配置参数，包含签名密钥、Cookie 名称、TTL 和最大续期次数。
#[derive(Debug, Clone, Serialize)]
pub struct SessionParams {
    /// Cookie 签名密钥，序列化时跳过（不暴露到外部）
    #[serde(skip)]
    key: Key,
    /// 存储 Session ID 的 Cookie 名称
    cookie: String,
    /// Session 有效期（秒），默认 86400（24 小时）
    ttl: i64,
    /// 允许续期的最大次数，0 表示不允许续期
    max_renewal: u8,
}

impl SessionParams {
    /// 以签名密钥创建 SessionParams，其余字段使用默认值（TTL 24h，不允许续期）。
    pub fn new(key: Key) -> Self {
        Self {
            key,
            cookie: String::new(),
            ttl: 24 * 60 * 60,
            max_renewal: 0,
        }
    }

    /// 设置存储 Session ID 的 Cookie 名称，支持链式调用。
    #[must_use]
    pub fn with_cookie(mut self, cookie: impl Into<String>) -> Self {
        self.cookie = cookie.into();
        self
    }

    /// 设置 Session 有效期（秒），支持链式调用。
    #[must_use]
    pub fn with_ttl(mut self, ttl: i64) -> Self {
        self.ttl = ttl;
        self
    }

    /// 设置允许续期的最大次数，支持链式调用。
    #[must_use]
    pub fn with_max_renewal(mut self, max_renewal: u8) -> Self {
        self.max_renewal = max_renewal;
        self
    }
}

/// Session 的内部数据，序列化后存入 Redis。
#[derive(Serialize, Deserialize, Default, Clone)]
struct SessionData {
    /// 用户 ID
    user_id: i64,
    /// Session 唯一标识（UUID）
    id: String,
    /// 签发时间戳（Unix 秒）
    iat: i64,
    /// 用户账号
    account: String,
    /// 已续期次数
    renewal_count: u8,
    /// 角色列表
    roles: Vec<String>,
    /// 用户组列表
    groups: Vec<String>,
}

/// HTTP Session，持有 Redis 缓存引用、配置参数和当前会话数据。
/// 实现了 axum `FromRequestParts`，可直接作为 handler 参数提取。
#[derive(Clone)]
pub struct Session {
    cache: &'static RedisCache,
    params: Arc<SessionParams>,
    data: SessionData,
}

impl Session {
    /// 创建未登录的空 Session，数据从下一次请求中按需加载。
    pub fn new(cache: &'static RedisCache, params: Arc<SessionParams>) -> Self {
        Self {
            cache,
            params,
            data: SessionData::default(),
        }
    }

    /// 生成 Redis 存储键，格式为 `ss:{session_id}`。
    fn get_key(id: &str) -> String {
        format!("ss:{id}")
    }

    /// 校验用户是否已登录，未登录时返回 401 错误。
    fn validate_login(&self) -> Result<()> {
        if !self.is_login() {
            return Err(Error::UserNotLogin.into());
        }
        Ok(())
    }

    /// 返回 `true` 表示用户已登录（account 非空）。
    pub fn is_login(&self) -> bool {
        !self.data.account.is_empty()
    }

    /// 返回 `true` 表示 Session 尚未达到最大续期次数，可以续期。
    pub fn can_renew(&self) -> bool {
        self.data.renewal_count < self.params.max_renewal
    }

    /// 设置账号和用户 ID，账号变更时自动生成新的 Session ID，支持链式调用。
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

    /// 设置角色列表，支持链式调用。
    #[must_use]
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.data.roles = roles;
        self
    }

    /// 设置用户组列表，支持链式调用。
    #[must_use]
    pub fn with_groups(mut self, groups: Vec<String>) -> Self {
        self.data.groups = groups;
        self
    }

    /// 续期：累加续期计数并更新签发时间戳。
    pub fn refresh(&mut self) {
        self.data.renewal_count += 1;
        self.data.iat = timestamp();
    }

    /// 返回当前登录的用户账号。
    pub fn get_account(&self) -> &str {
        &self.data.account
    }

    /// 返回当前登录的用户 ID。
    pub fn get_user_id(&self) -> i64 {
        self.data.user_id
    }

    /// 返回 Session 过期时间的格式化字符串。
    pub fn get_expired_at(&self) -> String {
        from_timestamp(self.data.iat + self.params.ttl, 0)
    }

    /// 返回 `true` 表示 Session 将在 1 小时内过期。
    pub fn is_will_expired(&self) -> bool {
        self.data.iat + self.params.ttl - timestamp() < 3600
    }

    /// 返回 Session 签发时间的格式化字符串。
    pub fn get_issued_at(&self) -> String {
        from_timestamp(self.data.iat, 0)
    }

    /// 返回 `true` 表示 Session 已超过 TTL 过期。
    pub fn is_expired(&self) -> bool {
        self.data.iat + self.params.ttl < timestamp()
    }

    /// 重置 Session（登出），清除 ID 和账号信息。
    pub fn reset(&mut self) {
        self.data.id = String::new();
        self.data.account = String::new();
    }

    /// 将当前 Session 数据持久化到 Redis，TTL 与配置一致。
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

/// 将 Session 转换为携带签名 Cookie 的 `SignedCookieJar`。
/// Session ID 为空时将 Cookie max-age 设为 0（即删除 Cookie）。
impl TryFrom<&Session> for SignedCookieJar {
    type Error = tibba_error::Error;

    fn try_from(se: &Session) -> Result<Self, Self::Error> {
        let mut c = CookieBuilder::new(se.params.cookie.clone(), se.data.id.clone())
            .path("/")
            .http_only(true)
            .max_age(time::Duration::seconds(se.params.ttl));

        if se.data.id.is_empty() {
            // ID 为空表示登出，将 max-age 置 0 以清除客户端 Cookie
            c = c.max_age(time::Duration::days(0));
        }

        Ok(SignedCookieJar::new(se.params.key.clone()).add(c))
    }
}

/// Session 登出/刷新接口的响应体。
#[derive(Debug, Serialize, Deserialize, Default)]
struct SessionResp {
    account: String,
    renewal_count: u8,
}

/// 将 Session 序列化为 HTTP 响应：设置签名 Cookie + JSON 账号信息。
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

/// 将 Session 和额外数据一起序列化为 HTTP 响应，同时设置签名 Cookie。
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

/// axum extractor：从请求扩展中提取 Session，按需从 Redis 加载数据。
/// 若 Cookie 中存在有效 Session ID 且 Redis 中有对应数据，则填充 SessionData。
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
        // iat == 0 表示本次请求尚未从 Redis 加载过数据
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
                // 回写到扩展，同一请求内后续提取无需再查 Redis
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

/// axum extractor：要求用户已登录，否则返回 401。
/// 通过 `Deref`/`DerefMut` 可直接访问内部 `Session` 的所有方法。
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

/// axum extractor：要求用户已登录且具有 Admin 或 SuperAdmin 角色，否则返回 401/403。
/// 通过 `Deref`/`DerefMut` 可直接访问内部 `Session` 的所有方法。
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
