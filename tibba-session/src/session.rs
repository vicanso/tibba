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

use super::{Error, LOG_TARGET, permission_grants};
use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::{CookieJar, Key, SignedCookieJar};
use cookie::{CookieBuilder, SameSite};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tibba_cache::{
    RedisCache, is_credential_revoked, revoke_role_credentials, revoke_user_credentials,
};
use tibba_runtime::CTX;
use tibba_util::{from_timestamp, is_development, timestamp, uuid};
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

/// 撤销标记的 TTL：与 Session TTL 一致——过了这个时间，被它针对的旧 Session 本身也已过期。
///
/// 同一套标记也被 JWT access token 校验复用（见 `tibba_cache::is_credential_revoked`），
/// 因此要求 Session TTL 不短于 JWT access TTL（默认 7 天 vs 15 分钟）。
fn revocation_ttl(params: &SessionParams) -> Duration {
    Duration::from_secs(params.ttl.max(1) as u64)
}

/// 使某用户**此前签发**的全部登录凭证（Session 与 JWT access token）失效。
///
/// Session 里缓存了登录时的 roles / groups / permissions，且 Redis 中没有
/// 「用户 → Session 列表」的索引。此前后台禁用账号、收回角色或用户重置密码后，
/// 已登录的会话仍按旧身份继续工作直到 TTL（默认 7 天）到期。
///
/// 这里写入一个撤销时刻，加载 Session 时凡 `iat` 早于该时刻的一律视为未登录，
/// 用户重新登录即拿到最新的角色与权限。
pub async fn revoke_user_sessions(
    cache: &RedisCache,
    params: &SessionParams,
    user_id: i64,
) -> Result<()> {
    revoke_user_credentials(cache, user_id, revocation_ttl(params)).await?;
    Ok(())
}

/// 使持有该角色的所有用户此前签发的登录凭证失效。
///
/// 用于收回角色的权限（撤销授予、删除权限点）：Session / JWT 中缓存的权限并集
/// 不会自动更新，受影响的用户需重新登录以拿到收窄后的权限。
pub async fn revoke_role_sessions(
    cache: &RedisCache,
    params: &SessionParams,
    role: &str,
) -> Result<()> {
    revoke_role_credentials(cache, role, revocation_ttl(params)).await?;
    Ok(())
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
    /// 该用户在本次 Session 内拥有的权限码并集（登录时根据 roles 翻译并缓存）。
    /// 用 `#[serde(default)]` 兼容旧版 Redis 中没有此字段的 Session blob。
    #[serde(default)]
    permissions: Vec<String>,
}

/// HTTP Session，持有 Redis 缓存引用、配置参数和当前会话数据。
/// 实现了 axum `FromRequestParts`，可直接作为 handler 参数提取。
#[derive(Clone)]
pub struct Session {
    cache: &'static RedisCache,
    params: Arc<SessionParams>,
    data: SessionData,
    /// `with_account` 轮换 ID 前的原会话 ID，供 `save` / `destroy` 清理旧 Redis 键。
    prev_id: Option<String>,
    /// 本次请求中会话被重置/销毁，需要在响应下发清除 Cookie（Max-Age=0）。
    cleared: bool,
}

impl Session {
    /// 创建未登录的空 Session，数据从下一次请求中按需加载。
    pub fn new(cache: &'static RedisCache, params: Arc<SessionParams>) -> Self {
        Self {
            cache,
            params,
            data: SessionData::default(),
            prev_id: None,
            cleared: false,
        }
    }

    /// 使当前 Session 所属用户此前签发的全部 Session 失效，见 [`revoke_user_sessions`]。
    pub async fn revoke_user(&self, user_id: i64) -> Result<()> {
        revoke_user_sessions(self.cache, &self.params, user_id).await
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

    /// 设置账号和用户 ID。每次登录/绑定账号始终轮换为新 Session ID（防会话固定攻击），
    /// 并保留旧 ID 供 `save()` / `destroy()` 删除 Redis 中遗留键。
    #[must_use]
    pub fn with_account(mut self, account: impl Into<String>, user_id: i64) -> Self {
        let account = account.into();
        if !self.data.id.is_empty() && self.prev_id.is_none() {
            self.prev_id = Some(self.data.id.clone());
        }
        self.data.id = uuid();
        self.data.account = account;
        self.data.user_id = user_id;
        self.data.iat = timestamp();
        self.cleared = false;
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

    /// 设置该用户本次 Session 内拥有的权限码集合，支持链式调用。
    /// 通常在登录时由路由层根据 `users.roles` 查 `role_permissions` 表得到。
    #[must_use]
    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.data.permissions = permissions;
        self
    }

    /// 返回该用户本次 Session 内的权限码列表（只读视图）。
    pub fn get_permissions(&self) -> &[String] {
        &self.data.permissions
    }

    /// 返回该用户本次 Session 内的角色列表（只读视图）。
    pub fn get_roles(&self) -> &[String] {
        &self.data.roles
    }

    /// 判断当前 Session 是否拥有 `required` 权限码。
    ///
    /// 通配规则：
    /// - `"*"` 命中任何 `required`
    /// - `"resource:*"` 命中所有 `resource:` 开头的 `required`（如 `"user:*"` 命中 `"user:read"`）
    /// - 其余必须精确相等
    ///
    /// 未登录直接返回 false。
    pub fn has_permission(&self, required: &str) -> bool {
        if !self.is_login() {
            return false;
        }
        permission_grants(&self.data.permissions, required)
    }

    /// 守卫式检查：未登录返回 401，已登录但缺权限返回 403。用于 handler 内手动鉴权。
    pub fn require_permission(&self, required: &str) -> Result<()> {
        self.validate_login()?;
        if permission_grants(&self.data.permissions, required) {
            Ok(())
        } else {
            Err(Error::PermissionDenied {
                required: required.to_string(),
            }
            .into())
        }
    }

    /// 是否持有 Admin 或 SuperAdmin 角色（与 [`AdminSession`] 提取器判定一致）。
    #[must_use]
    pub fn is_admin(&self) -> bool {
        self.data.roles.iter().any(|role| {
            let r = Role::from(role.as_str());
            r == Role::Admin || r == Role::SuperAdmin
        })
    }

    /// 要求 Admin / SuperAdmin 角色，否则 403。
    pub fn require_admin(&self) -> Result<()> {
        self.validate_login()?;
        if self.is_admin() {
            Ok(())
        } else {
            Err(Error::UserNotAdmin.into())
        }
    }

    /// 续期：饱和累加续期计数并更新签发时间戳。
    pub fn refresh(&mut self) {
        self.data.renewal_count = self.data.renewal_count.saturating_add(1);
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
        from_timestamp(self.data.iat.saturating_add(self.params.ttl), 0)
    }

    /// 返回 `true` 表示 Session 将在 1 小时内过期。
    pub fn is_will_expired(&self) -> bool {
        self.data
            .iat
            .saturating_add(self.params.ttl)
            .saturating_sub(timestamp())
            < 3600
    }

    /// 返回 Session 签发时间的格式化字符串。
    pub fn get_issued_at(&self) -> String {
        from_timestamp(self.data.iat, 0)
    }

    /// 返回 `true` 表示 Session 已超过 TTL 过期。
    pub fn is_expired(&self) -> bool {
        self.data.iat <= 0 || self.data.iat.saturating_add(self.params.ttl) <= timestamp()
    }

    /// 重置 Session（登出），清空所有数据并标记清除客户端 Cookie。
    /// 只清 `id` / `account` 会留下 `user_id` / `roles` / `groups` /
    /// `renewal_count` / `iat` 等残留字段，后续读取会拿到上一登录态的脏数据。
    pub fn reset(&mut self) {
        self.data = SessionData::default();
        self.cleared = true;
    }

    /// 彻底销毁 Session：删除 Redis 中当前与轮换前的会话键，重置本地状态并清除 Cookie。
    pub async fn destroy(&mut self) -> Result<()> {
        if !self.data.id.is_empty() {
            self.cache.del(&Self::get_key(&self.data.id)).await?;
        }
        if let Some(prev) = self.prev_id.take()
            && prev != self.data.id
        {
            let _ = self.cache.del(&Self::get_key(&prev)).await;
        }
        self.reset();
        Ok(())
    }

    /// 将当前 Session 数据持久化到 Redis，TTL 与配置一致；若发生过 ID 轮换则顺手删除旧键。
    pub async fn save(&mut self) -> Result<()> {
        if self.data.id.is_empty() {
            return Err(Error::SessionIdEmpty.into());
        }
        let ttl_secs = u64::try_from(self.params.ttl.max(1)).unwrap_or(1);
        self.cache
            .set_struct(
                &Self::get_key(&self.data.id),
                &self.data,
                Some(Duration::from_secs(ttl_secs)),
            )
            .await?;
        if let Some(prev) = self.prev_id.take()
            && prev != self.data.id
        {
            let _ = self.cache.del(&Self::get_key(&prev)).await;
        }
        Ok(())
    }
}

/// 将 Session 与响应体组装为携带 Cookie 的完整响应。
/// Session 已销毁或 ID 为空时使用无签名清除 Cookie（`Max-Age=0`）。
fn with_session_cookie<T: IntoResponse>(se: &Session, body: T) -> Response {
    if se.cleared || se.data.id.is_empty() {
        let removal = CookieBuilder::new(se.params.cookie.clone(), "")
            .path("/")
            .http_only(true)
            .same_site(SameSite::Lax)
            .secure(!is_development())
            .max_age(time::Duration::ZERO)
            .build();
        return (CookieJar::new().add(removal), body).into_response();
    }

    let c = CookieBuilder::new(se.params.cookie.clone(), se.data.id.clone())
        .path("/")
        .http_only(true)
        // SameSite=Lax：跨站请求不携带会话，缓解 CSRF，同时不影响顶层导航保持登录态。
        // 生产环境强制 Secure，禁止明文 HTTP 传输会话；dev 允许 http 调试。
        .same_site(SameSite::Lax)
        .secure(!is_development())
        .max_age(time::Duration::seconds(se.params.ttl));

    (SignedCookieJar::new(se.params.key.clone()).add(c), body).into_response()
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
        let resp = Json(SessionResp {
            account: self.data.account.clone(),
            renewal_count: self.data.renewal_count,
        });
        with_session_cookie(&self, resp)
    }
}

/// 将 Session 和额外数据一起序列化为 HTTP 响应，同时设置签名 Cookie。
pub struct SessionResponse<T>(pub Session, pub T);

impl<T> IntoResponse for SessionResponse<T>
where
    T: IntoResponse,
{
    fn into_response(self) -> Response {
        with_session_cookie(&self.0, self.1)
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
                debug!(
                    target: LOG_TARGET,
                    len = session_id.len(),
                    "ignore malformed session cookie length; downgrade to guest"
                );
                return Ok(se);
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
                // 已超过 TTL 过期时按匿名访客降级处理
                if data.iat <= 0 || data.iat.saturating_add(se.params.ttl) <= timestamp() {
                    debug!(
                        target: LOG_TARGET,
                        id = data.id,
                        "session expired; downgrade to guest"
                    );
                    return Ok(se);
                }
                // 已被撤销（禁用 / 改角色 / 收回角色权限 / 重置密码之后）的会话按未登录处理
                if !data.account.is_empty()
                    && is_credential_revoked(se.cache, data.user_id, &data.roles, data.iat).await?
                {
                    debug!(
                        target: LOG_TARGET,
                        id = data.id,
                        user_id = data.user_id,
                        "session revoked"
                    );
                    return Ok(se);
                }
                se.data = data;
                // 回写到扩展，同一请求内后续提取无需再查 Redis
                parts.extensions.insert(se.clone());
                if se.is_login() {
                    let _ = CTX.try_with(|ctx| ctx.set_account(se.get_account()));
                }

                return Ok(se);
            }
        }
        if se.is_login() {
            let _ = CTX.try_with(|ctx| ctx.set_account(se.get_account()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Request, header};
    use pretty_assertions::assert_eq;
    use std::sync::OnceLock;
    use tibba_cache::new_redis_client;
    use tibba_config::Config;
    use tibba_error::Error as BaseError;

    fn test_cache() -> &'static RedisCache {
        static CACHE: OnceLock<RedisCache> = OnceLock::new();
        CACHE.get_or_init(|| {
            let cfg = Config::builder()
                .add_toml("uri = \"redis://127.0.0.1:6379\"")
                .build()
                .expect("config");
            let client = Box::leak(Box::new(new_redis_client(&cfg).expect("redis client")));
            RedisCache::new(client)
        })
    }

    fn test_params() -> Arc<SessionParams> {
        let key = Key::from(&[0x42u8; 64]);
        Arc::new(
            SessionParams::new(key)
                .with_cookie("tibba_sid")
                .with_ttl(3600)
                .with_max_renewal(2),
        )
    }

    #[test]
    fn with_account_rotates_id_and_tracks_prev_id() {
        let params = test_params();
        let s1 = Session::new(test_cache(), params).with_account("alice", 1);
        let id1 = s1.data.id.clone();
        assert!(!id1.is_empty());
        assert_eq!(s1.prev_id, None);

        // 同账号再次登录也必须轮换新 Session ID，并记录原 ID 待清理
        let s2 = s1.with_account("alice", 1);
        assert_ne!(s2.data.id, id1);
        assert_eq!(s2.prev_id.as_deref(), Some(id1.as_str()));
    }

    #[test]
    fn cleared_or_empty_session_emits_unsigned_max_age_zero_cookie() {
        let params = test_params();
        let mut s = Session::new(test_cache(), params).with_account("alice", 1);
        s.reset();
        let resp = s.into_response();
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .expect("Set-Cookie present");
        assert!(set_cookie.starts_with("tibba_sid=;"), "{set_cookie}");
        assert!(set_cookie.contains("Max-Age=0"), "{set_cookie}");
    }

    #[test]
    fn refresh_saturates_and_error_sub_categories_are_stable() {
        let params = test_params();
        let mut s = Session::new(test_cache(), params).with_account("alice", 1);
        s.data.renewal_count = u8::MAX;
        s.refresh();
        assert_eq!(s.data.renewal_count, u8::MAX);

        let e1 = BaseError::from(Error::UserNotLogin);
        assert_eq!(e1.status(), 401);
        assert_eq!(e1.sub_category(), Some("not_logged_in"));

        let e2 = BaseError::from(Error::UserNotAdmin);
        assert_eq!(e2.status(), 403);
        assert_eq!(e2.sub_category(), Some("not_admin"));
    }

    #[tokio::test]
    async fn malformed_short_session_cookie_downgrades_to_guest() {
        let params = test_params();
        let seed = Session::new(test_cache(), params.clone());
        // 构造合法签名的短 value（< 36 字符）cookie
        let jar = SignedCookieJar::new(params.key.clone())
            .add(CookieBuilder::new("tibba_sid", "too-short").build());
        let resp_with_cookie = jar.into_response();
        let signed_cookie_pair = resp_with_cookie
            .headers()
            .get(header::SET_COOKIE)
            .expect("Set-Cookie header")
            .to_str()
            .expect("valid header str")
            .split(';')
            .next()
            .expect("cookie key=val")
            .to_string();

        let mut req = Request::builder()
            .method(Method::GET)
            .uri("/test")
            .header(header::COOKIE, signed_cookie_pair)
            .body(())
            .expect("request build");
        req.extensions_mut().insert(seed);
        let (mut parts, _) = req.into_parts();

        let extracted = Session::from_request_parts(&mut parts, &())
            .await
            .expect("短 session cookie 应降级为匿名访客，不返回 500");
        assert!(!extracted.is_login());
    }
}
