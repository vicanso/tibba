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

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use snafu::{OptionExt, Snafu};
use sqlx::PgPool;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tibba_cache::RedisCache;
use tibba_email::EmailConfig;
use tibba_error::Error as BaseError;
use tibba_oauth::OAuthConfig;
use tibba_middleware::{ClientIp, RequestId, user_tracker, validate_captcha};
use tibba_model::{Model, ROLE_SUPER_ADMIN, User, UserModel, UserUpdateParams};
use tibba_model_builtin::{AuditLogModel, AuditLogParams, RolePermissionModel};
use tibba_session::{Session, SessionResponse, UserSession};
use tibba_util::{
    JsonParams, JsonResult, generate_device_id_cookie, get_device_id_from_cookie, is_development,
    is_test, now, sha256, timestamp, timestamp_hash, uuid, validate_timestamp_hash,
};
use tibba_validator::*;
use utoipa::{OpenApi, ToSchema};
use validator::Validate;

mod api_key;
mod email_verify;
mod jwt_auth;
mod login_guard;
mod oauth_github;
mod oauth_google;
mod password_reset;
mod totp;

// API Key 鉴权中间件，供主 crate 全局挂载（session 中间件之后）。
pub use api_key::api_key_auth;

/// 注册成功回调的 Future 类型
pub type OnRegisterFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
/// 注册成功后的回调。参数为新用户 ID（i64）。
pub type OnRegisterFn = Arc<dyn Fn(i64) -> OnRegisterFuture + Send + Sync>;

/// 注册 handler 的 axum 状态，包含 DB pool 和可选的注册后回调。
#[derive(Clone)]
struct RegisterState {
    pool: &'static PgPool,
    on_register: Option<OnRegisterFn>,
}

/// 模块对外仍以 `tibba_error::Error` 为错误类型，本地 `Error` 仅作 snafu 上下文。
type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "user_router";

/// 用户路由模块内部错误，统一通过 `From` 转换为 `tibba_error::Error`。
#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// 登录令牌过期或时间漂移超过 ±60 秒（HTTP 401）
    #[snafu(display("timestamp is expired"))]
    TokenExpired,

    /// 账号或密码错误。统一文案，避免账号枚举攻击（HTTP 401）
    #[snafu(display("account or password is wrong"))]
    BadCredentials,

    /// Session 中的账号在数据库中已不存在，强制客户端重登（HTTP 401）
    #[snafu(display("user not found: {account}"))]
    UserNotFound { account: String },

    /// 登录失败次数过多，账号或来源 IP 被临时锁定（HTTP 429）
    #[snafu(display("too many failed login attempts, please retry later"))]
    TooManyAttempts,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::TokenExpired => BaseError::new("timestamp is expired")
                .with_sub_category("token_expired")
                .with_status(401)
                .with_exception(false),
            Error::BadCredentials => BaseError::new("account or password is wrong")
                .with_sub_category("bad_credentials")
                .with_status(401)
                .with_exception(false),
            Error::UserNotFound { account } => BaseError::new(format!("user not found: {account}"))
                .with_sub_category("user_not_found")
                .with_status(401)
                .with_exception(false),
            Error::TooManyAttempts => {
                BaseError::new("too many failed login attempts, please retry later")
                    .with_sub_category("too_many_attempts")
                    .with_status(429)
                    .with_exception(false)
            }
        };
        err.with_category(ERROR_CATEGORY)
    }
}

/// 登录令牌接口的响应结构体。
#[derive(Serialize, ToSchema)]
struct LoginTokenResp {
    /// 服务端生成时的 Unix 时间戳（秒），用于防重放校验。
    ts: i64,
    /// 对 `token` 和 `secret` 的 HMAC 签名，供客户端验证服务端身份。
    hash: String,
    /// 一次性随机令牌，客户端须在登录请求中携带。
    token: String,
}

/// 生成登录令牌，返回 `{ ts, hash, token }`。
/// 客户端须在 60 秒内使用该令牌完成登录，超时后服务端拒绝。
#[utoipa::path(
    get,
    path = "/users/login/token",
    tag = "user",
    responses((status = 200, description = "防重放登录令牌", body = LoginTokenResp))
)]
async fn login_token(State(secret): State<String>) -> JsonResult<LoginTokenResp> {
    let token = uuid();
    let (ts, hash) = timestamp_hash(&token, &secret);

    Ok(Json(LoginTokenResp { ts, hash, token }))
}

/// 登录请求参数，含防重放令牌和用户凭据。
/// `pub(crate)` 因 jwt_auth 模块需要复用同样的校验链，避免前端写两套适配。
#[derive(Deserialize, Validate, Debug, ToSchema)]
pub(crate) struct LoginParams {
    /// 客户端从 `/login/token` 获取的时间戳
    pub ts: i64,
    /// 客户端从 `/login/token` 获取的一次性令牌（UUID 格式）
    #[validate(custom(function = "x_uuid"))]
    pub token: String,
    /// 服务端对 `token` 的签名，用于校验令牌合法性（SHA-256 格式）
    #[validate(custom(function = "x_sha256"))]
    pub hash: String,
    /// 用户账号
    #[validate(custom(function = "x_user_account"))]
    pub account: String,
    /// 经过 `sha256(hash:password)` 处理后的密码
    #[validate(custom(function = "x_user_password"))]
    pub password: String,
}

impl LoginParams {
    /// 校验登录令牌：
    /// - 开发/测试环境下 `ts <= 0` 时跳过校验；
    /// - 时间戳偏差超过 60 秒时拒绝（防重放）；
    /// - 验证 HMAC 签名是否匹配。
    pub(crate) fn validate_token(&self, secret: &str) -> Result<()> {
        // 开发/测试环境允许跳过时间戳校验
        if self.ts <= 0 && (is_development() || is_test()) {
            return Ok(());
        }
        if (self.ts - timestamp()).abs() > 60 {
            return Err(Error::TokenExpired.into());
        }
        validate_timestamp_hash(self.ts, &self.token, &self.hash, secret)?;
        Ok(())
    }
}

/// `/me` 及登录接口的用户信息响应体。
#[derive(Debug, Clone, Serialize, Default, ToSchema)]
struct UserMeResp {
    /// 用户账号
    account: String,
    /// Session 过期时间（RFC3339 格式）
    expired_at: String,
    /// Session 签发时间（RFC3339 格式）
    issued_at: String,
    /// 服务端当前时间（RFC3339 格式）
    time: String,
    /// Session 是否可续期
    can_renew: bool,
    /// 昵称（可选）
    nickname: Option<String>,
    /// 手机号（可选）
    phone: Option<String>,
    /// 邮箱（可选）
    email: Option<String>,
    /// 头像 URL（可选）
    avatar: Option<String>,
    /// 角色列表（可选）
    roles: Option<Vec<String>>,
    /// 用户组列表（可选）
    groups: Option<Vec<String>>,
    /// 权限码列表，前端可据此控制按钮可见性
    permissions: Option<Vec<String>>,
}

/// 2FA 登录挑战响应：密码已过但需第二步验证时返回，**不**建立 Session。
#[derive(Serialize, ToSchema)]
struct LoginChallenge {
    /// 恒为 `true`，客户端据此进入第二步（调用 `/login/mfa`）。
    mfa_required: bool,
    /// 一次性挑战令牌（5 分钟有效），与动态码一起提交完成登录。
    mfa_token: String,
}

/// 用户登录接口。
/// 校验令牌合法性后，比对密码（`sha256(hash:stored_password)`）。
/// 若用户已启用 2FA，则返回 [`LoginChallenge`] 而不建立 Session；
/// 否则直接创建 Session 并返回用户信息。
#[utoipa::path(
    post,
    path = "/users/login",
    tag = "user",
    request_body = LoginParams,
    responses(
        (status = 200, description = "登录成功返回用户信息（UserMeResp）；已启用 2FA 时返回 { mfa_required, mfa_token }"),
        (status = 401, description = "账号或密码错误 / 令牌过期")
    )
)]
async fn login(
    State((secret, pool, cache)): State<(String, &'static PgPool, &'static RedisCache)>,
    session: Session,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<LoginParams>,
) -> Result<Response> {
    params.validate_token(&secret)?;
    let account = params.account;
    let ip_str = ip.to_string();

    // 暴力破解闸门：账号 / IP 在窗口内失败过多则直接 429，连密码都不再比对
    login_guard::ensure_not_locked(cache, &account, &ip_str).await?;

    let Some(user) = UserModel::new().get_by_account(pool, &account).await? else {
        login_guard::record_failure(cache, &account, &ip_str).await;
        return Err(Error::BadCredentials.into());
    };

    // 密码验证：sha256(hash:存储密码) 须与客户端传入的 password 相等
    let msg = format!("{}:{}", params.hash, user.password);
    if sha256(msg.as_bytes()) != params.password {
        login_guard::record_failure(cache, &account, &ip_str).await;
        return Err(Error::BadCredentials.into());
    }

    // 凭证正确：清账号失败计数（IP 计数保留，到期自动解锁）
    login_guard::clear_failures(cache, &account).await;

    // 2FA 闸门：已启用则不建会话，签发挑战令牌要求第二步
    let totp = UserModel::new().get_totp_state(pool, user.id).await?;
    if totp.enabled {
        let mfa_token =
            totp::create_mfa_challenge(cache, totp::MFA_PREFIX_SESSION, user.id).await?;
        return Ok(Json(LoginChallenge {
            mfa_required: true,
            mfa_token,
        })
        .into_response());
    }

    let resp =
        establish_session(session, user, pool, &request_id, ip.to_string(), &headers, "user.login")
            .await?;
    Ok(resp.into_response())
}

/// 密码（及可能的二步）校验通过后建立 Session 并返回用户信息响应。
/// `action` 用于审计区分 `user.login` / `user.login_mfa`。`login` 与
/// `login_mfa` 共用本助手，避免会话建立逻辑两处漂移。
async fn establish_session(
    session: Session,
    user: User,
    pool: &'static PgPool,
    request_id: &RequestId,
    ip: String,
    headers: &HeaderMap,
    action: &'static str,
) -> Result<SessionResponse<Json<UserMeResp>>> {
    let account = user.account.clone();
    let groups = user.groups.clone().unwrap_or_default();
    let roles = user.roles.clone().unwrap_or_default();

    // 把 roles 翻译为权限码并集，缓存到 Session（避免每次 handler 重查 DB）。
    // 失败仅记录但不阻断登录——RBAC 还未铺开时 role_permissions 可能为空。
    let permissions = RolePermissionModel::new()
        .list_permissions_for_roles(pool, &roles)
        .await
        .unwrap_or_default();

    let session = session
        .with_account(&account, user.id)
        .with_groups(groups)
        .with_roles(roles)
        .with_permissions(permissions.clone());
    session.save().await?;

    // 异步更新最后登录时间，失败不影响登录流程
    let _ = UserModel::new().update_last_login_at(pool, &account).await;

    // 审计：记录登录成功事件。失败仅日志（审计不能阻断业务）
    let _ = AuditLogModel::new()
        .log(
            pool,
            AuditLogParams::new(action)
                .with_user(user.id)
                .with_target("user", user.id.to_string())
                .with_request(request_id.as_str(), ip, user_agent_of(headers)),
        )
        .await;

    let info = UserMeResp {
        account,
        expired_at: session.get_expired_at(),
        issued_at: session.get_issued_at(),
        time: now(),
        can_renew: session.can_renew(),
        nickname: user.nickname,
        phone: user.phone,
        email: user.email,
        avatar: user.avatar,
        roles: user.roles,
        groups: user.groups,
        permissions: Some(permissions),
    };

    Ok(SessionResponse(session, Json(info)))
}

/// 第二步登录参数：挑战令牌 + 动态码/恢复码。
#[derive(Debug, Deserialize, Validate, ToSchema)]
struct LoginMfaParams {
    /// `/login` 返回的一次性挑战令牌（UUID）
    #[validate(custom(function = "x_uuid"))]
    mfa_token: String,
    /// TOTP 动态码（6 位）或恢复码（`xxxxx-xxxxx`）
    #[validate(length(min = 6, max = 32))]
    code: String,
}

/// `POST /login/mfa` —— 完成 2FA 登录第二步。
/// 挑战令牌为一次性：无论校验成败都已消费，验证失败需重新 `/login`。
#[utoipa::path(
    post,
    path = "/users/login/mfa",
    tag = "user",
    request_body = LoginMfaParams,
    responses(
        (status = 200, description = "二步通过，建立 Session 并返回用户信息", body = UserMeResp),
        (status = 401, description = "挑战令牌失效或动态码/恢复码错误")
    )
)]
async fn login_mfa(
    State((secret, pool, cache)): State<(String, &'static PgPool, &'static RedisCache)>,
    session: Session,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<LoginMfaParams>,
) -> Result<SessionResponse<Json<UserMeResp>>> {
    let user_id = totp::consume_mfa_challenge(cache, totp::MFA_PREFIX_SESSION, &params.mfa_token)
        .await?
        .ok_or(totp::Error::InvalidChallenge)?;

    if !totp::verify_second_factor(pool, &secret, user_id, params.code.trim()).await? {
        return Err(totp::Error::BadCode.into());
    }

    let user = UserModel::new()
        .get_by_id(pool, user_id as u64)
        .await?
        .context(UserNotFoundSnafu {
            account: user_id.to_string(),
        })?;

    establish_session(
        session,
        user,
        pool,
        &request_id,
        ip.to_string(),
        &headers,
        "user.login_mfa",
    )
    .await
}

/// 获取当前登录用户信息。
/// 若 Cookie 中无 device_id，则自动生成并写入；
/// 未登录时返回空的 `UserMeResp`。
#[utoipa::path(
    get,
    path = "/users/me",
    tag = "user",
    responses((status = 200, description = "当前用户信息；未登录时各字段为空", body = UserMeResp))
)]
async fn me(
    State(pool): State<&'static PgPool>,
    mut jar: CookieJar,
    session: Session,
) -> Result<(CookieJar, Json<UserMeResp>)> {
    let account = session.get_account();
    // 首次访问时生成设备 ID Cookie，用于前端设备追踪
    if get_device_id_from_cookie(&jar).is_none() {
        jar = jar.add(generate_device_id_cookie());
    }
    if !session.is_login() {
        return Ok((jar, Json(UserMeResp::default())));
    }
    let user = UserModel::new()
        .get_by_account(pool, account)
        .await?
        .context(UserNotFoundSnafu {
            account: account.to_string(),
        })?;
    let info = UserMeResp {
        account: account.to_string(),
        expired_at: session.get_expired_at(),
        issued_at: session.get_issued_at(),
        time: now(),
        can_renew: session.can_renew(),
        nickname: user.nickname,
        phone: user.phone,
        email: user.email,
        avatar: user.avatar,
        roles: user.roles,
        groups: user.groups,
        permissions: Some(session.get_permissions().to_vec()),
    };

    Ok((jar, Json(info)))
}

/// 注册请求参数。
#[derive(Deserialize, Validate, ToSchema)]
struct RegisterParams {
    #[validate(custom(function = "x_user_account"))]
    account: String,
    #[validate(custom(function = "x_user_password"))]
    password: String,
}

/// 注册成功响应体。
#[derive(Serialize, ToSchema)]
struct RegisterResp {
    /// 新用户 ID
    id: u64,
    /// 注册账号
    account: String,
}

/// 注册新用户接口。
/// 第一个注册成功的用户（id=1）自动授予超级管理员角色。
/// 注册成功后若配置了 `on_register` 回调，则异步触发；回调失败不影响注册结果。
#[utoipa::path(
    post,
    path = "/users/register",
    tag = "user",
    request_body = RegisterParams,
    responses((status = 200, description = "注册成功，返回新用户 id 与账号", body = RegisterResp))
)]
async fn register(
    State(state): State<RegisterState>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<RegisterParams>,
) -> JsonResult<RegisterResp> {
    let model = UserModel::new();
    let id = model
        .register(state.pool, &params.account, &params.password)
        .await?;
    // 首个用户自动升级为超级管理员
    if id == 1 {
        model
            .update_by_id(state.pool, id, json!({ "roles": [ROLE_SUPER_ADMIN] }))
            .await?;
    }
    if let Some(cb) = &state.on_register {
        cb(id as i64).await;
    }

    // 审计：记录注册事件
    let _ = AuditLogModel::new()
        .log(
            state.pool,
            AuditLogParams::new("user.register")
                .with_user(id as i64)
                .with_target("user", id.to_string())
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(&headers)),
        )
        .await;

    Ok(Json(RegisterResp {
        id,
        account: params.account,
    }))
}

/// 刷新 Session 有效期。仅当 Session 满足续期条件时才执行续期操作。
#[utoipa::path(
    patch,
    path = "/users/refresh",
    tag = "user",
    responses((status = 200, description = "已续期（或不满足续期条件时原样返回）"))
)]
async fn refresh_session(mut session: UserSession) -> Result<Session> {
    if !session.can_renew() {
        return Ok(session.into());
    }
    session.refresh();
    session.save().await?;
    Ok(session.into())
}

/// 登出接口，清除当前 Session。审计 reset 之前抓 user_id，未登录态不记。
#[utoipa::path(
    delete,
    path = "/users/logout",
    tag = "user",
    responses((status = 200, description = "已登出，Session 已清空"))
)]
async fn logout(
    State(pool): State<&'static PgPool>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    mut session: Session,
) -> Session {
    let user_id = session.get_user_id();
    if user_id != 0 {
        let _ = AuditLogModel::new()
            .log(
                pool,
                AuditLogParams::new("user.logout")
                    .with_user(user_id)
                    .with_target("user", user_id.to_string())
                    .with_request(
                        request_id.as_str(),
                        ip.to_string(),
                        user_agent_of(&headers),
                    ),
            )
            .await;
    }
    session.reset();
    session
}

/// 更新用户资料的请求参数（所有字段均为可选）。
#[derive(Deserialize, Validate, ToSchema)]
struct UpdateProfileParams {
    #[validate(length(max = 100))]
    nickname: Option<String>,
    #[validate(length(max = 64))]
    phone: Option<String>,
    #[validate(custom(function = "x_user_email"))]
    email: Option<String>,
    #[validate(url)]
    avatar: Option<String>,
}

/// 更新当前登录用户的个人资料（邮箱、头像），成功返回 204 No Content。
#[utoipa::path(
    patch,
    path = "/users/profile",
    tag = "user",
    request_body = UpdateProfileParams,
    responses((status = 204, description = "资料更新成功"))
)]
async fn update_profile(
    State(pool): State<&'static PgPool>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<UpdateProfileParams>,
) -> Result<StatusCode> {
    let account = session.get_account();
    // 审计 detail：记录这次改了哪几个字段（不带具体值，避免审计行膨胀 / 泄敏）
    let mut changed: Vec<&'static str> = Vec::new();
    if params.nickname.is_some() {
        changed.push("nickname");
    }
    if params.phone.is_some() {
        changed.push("phone");
    }
    if params.email.is_some() {
        changed.push("email");
    }
    if params.avatar.is_some() {
        changed.push("avatar");
    }
    let update = UserUpdateParams {
        nickname: params.nickname,
        phone: params.phone,
        email: params.email,
        avatar: params.avatar,
        ..Default::default()
    };
    UserModel::new()
        .update_by_account(pool, account, update)
        .await?;

    let user_id = session.get_user_id();
    let _ = AuditLogModel::new()
        .log(
            pool,
            AuditLogParams::new("user.profile_update")
                .with_user(user_id)
                .with_target("user", user_id.to_string())
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(&headers))
                .with_detail(json!({ "changed": changed })),
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 构建用户路由所需的参数。
pub struct UserRouterParams {
    /// 用于登录令牌签名的密钥
    pub secret: String,
    /// 验证码魔法码（用于开发/测试环境跳过验证码校验）
    pub magic_code: String,
    /// 数据库连接池
    pub pool: &'static PgPool,
    /// Redis 缓存（存储验证码答案、邮箱验证 token、密码重置 token）
    pub cache: &'static RedisCache,
    /// 全局邮件配置——被邮箱验证 / 密码重置端点用来构造 EmailService
    pub email_config: &'static EmailConfig,
    /// 全局 OAuth 配置——被 `/oauth/github/*` 端点使用
    pub oauth_config: &'static OAuthConfig,
    /// OAuth 登录成功后跳回的前端地址；空串时跳 `/`
    pub oauth_success_redirect: String,
    /// 注册成功后的回调，可选。失败不影响注册流程。
    pub on_register: Option<OnRegisterFn>,
}

/// 创建用户相关路由，包含以下端点：
/// - `GET  /login/token`              — 获取登录令牌（带频率限制中间件）
/// - `POST /login`                    — 用户登录（带验证码校验 + 频率限制中间件）
/// - `GET  /me`                       — 获取当前用户信息
/// - `PATCH /refresh`                 — 刷新 Session 有效期
/// - `POST /register`                 — 注册新用户（带频率限制中间件）
/// - `DELETE /logout`                 — 登出（带频率限制中间件）
/// - `PATCH /profile`                 — 更新个人资料
/// - `POST /email/verify/request`     — 登录态触发邮箱验证（发送验证码）
/// - `POST /email/verify/confirm`     — 用 token 完成邮箱验证
/// - `POST /password/reset/request`   — 匿名按账号请求重置（总是返回 204）
/// - `POST /password/reset/confirm`   — 用 token + 新密码完成重置
/// - `POST /login/mfa`                — 2FA 登录第二步（Session 路径）
/// - `POST /totp/enroll`              — 生成 TOTP 密钥（返回 otpauth URI，待激活）
/// - `POST /totp/activate`            — 验码激活 2FA，返回一次性恢复码
/// - `POST /totp/disable`             — 验码关闭 2FA
/// - `GET  /totp/status`              — 查询 2FA 状态
/// - `POST /login/jwt/mfa`            — 2FA 登录第二步（JWT 路径）
pub fn new_user_router(params: UserRouterParams) -> Router {
    let name = "user";

    let email_verify_state = email_verify::EmailVerifyState {
        pool: params.pool,
        cache: params.cache,
        email_config: params.email_config,
    };
    let password_reset_state = password_reset::PasswordResetState {
        pool: params.pool,
        cache: params.cache,
        email_config: params.email_config,
    };
    let oauth_github_state = oauth_github::OauthGitHubState {
        pool: params.pool,
        cache: params.cache,
        oauth_config: params.oauth_config,
        success_redirect: params.oauth_success_redirect.clone(),
    };
    let oauth_google_state = oauth_google::OauthGoogleState {
        pool: params.pool,
        cache: params.cache,
        oauth_config: params.oauth_config,
        success_redirect: params.oauth_success_redirect,
    };
    let jwt_auth_state = jwt_auth::JwtAuthState {
        pool: params.pool,
        cache: params.cache,
        secret: params.secret.clone(),
    };
    let totp_state = totp::TotpRouterState {
        pool: params.pool,
        secret: params.secret.clone(),
    };

    Router::new()
        .route(
            "/login/token",
            get(login_token)
                .with_state(params.secret.clone())
                .layer(from_fn_with_state(
                    (name, "login_token").into(),
                    user_tracker,
                )),
        )
        .route(
            "/login",
            post(login)
                .with_state((params.secret.clone(), params.pool, params.cache))
                .layer(from_fn_with_state(
                    (params.magic_code, params.cache),
                    validate_captcha,
                ))
                .layer(from_fn_with_state((name, "login").into(), user_tracker)),
        )
        // 2FA 登录第二步：凭挑战令牌 + 动态码完成（带频率限制，无需再过验证码）
        .route(
            "/login/mfa",
            post(login_mfa)
                .with_state((params.secret.clone(), params.pool, params.cache))
                .layer(from_fn_with_state((name, "login_mfa").into(), user_tracker)),
        )
        .route("/me", get(me).with_state(params.pool))
        .route("/refresh", patch(refresh_session))
        .route(
            "/register",
            post(register)
                .with_state(RegisterState {
                    pool: params.pool,
                    on_register: params.on_register,
                })
                .layer(from_fn_with_state((name, "register").into(), user_tracker)),
        )
        .route(
            "/logout",
            delete(logout)
                .with_state(params.pool)
                .layer(from_fn_with_state((name, "logout").into(), user_tracker)),
        )
        .route("/profile", patch(update_profile).with_state(params.pool))
        // API Key / 个人访问令牌（PAT）：登录态自助管理；明文令牌仅创建时返回一次
        .route(
            "/api-keys",
            post(api_key::create_api_key)
                .get(api_key::list_api_keys)
                .with_state(params.pool),
        )
        .route(
            "/api-keys/{id}",
            delete(api_key::revoke_api_key).with_state(params.pool),
        )
        // 邮箱验证（登录态调用，request 端点带 user_tracker 防刷）
        .route(
            "/email/verify/request",
            post(email_verify::request_verify)
                .with_state(email_verify_state.clone())
                .layer(from_fn_with_state(
                    (name, "email_verify_request").into(),
                    user_tracker,
                )),
        )
        .route(
            "/email/verify/confirm",
            post(email_verify::confirm_verify).with_state(email_verify_state),
        )
        // 密码重置（匿名调用，request 端点强烈防刷以缓解枚举/扫描）
        .route(
            "/password/reset/request",
            post(password_reset::request_reset)
                .with_state(password_reset_state.clone())
                .layer(from_fn_with_state(
                    (name, "password_reset_request").into(),
                    user_tracker,
                )),
        )
        .route(
            "/password/reset/confirm",
            post(password_reset::confirm_reset)
                .with_state(password_reset_state)
                .layer(from_fn_with_state(
                    (name, "password_reset_confirm").into(),
                    user_tracker,
                )),
        )
        // GitHub OAuth：start 生成 state 跳 GitHub；callback 拿 code 后落地用户 + 建 Session
        .route(
            "/oauth/github/start",
            get(oauth_github::start_login)
                .with_state(oauth_github_state.clone())
                .layer(from_fn_with_state(
                    (name, "oauth_github_start").into(),
                    user_tracker,
                )),
        )
        .route(
            "/oauth/github/callback",
            get(oauth_github::callback)
                .with_state(oauth_github_state)
                .layer(from_fn_with_state(
                    (name, "oauth_github_callback").into(),
                    user_tracker,
                )),
        )
        // Google OAuth：与 GitHub 对称，差异在 provider endpoint / userinfo JSON
        .route(
            "/oauth/google/start",
            get(oauth_google::start_login)
                .with_state(oauth_google_state.clone())
                .layer(from_fn_with_state(
                    (name, "oauth_google_start").into(),
                    user_tracker,
                )),
        )
        .route(
            "/oauth/google/callback",
            get(oauth_google::callback)
                .with_state(oauth_google_state)
                .layer(from_fn_with_state(
                    (name, "oauth_google_callback").into(),
                    user_tracker,
                )),
        )
        // TOTP 两步验证（均需登录态）：enroll 生成密钥 → activate 激活 → disable 关闭
        .route(
            "/totp/enroll",
            post(totp::enroll)
                .with_state(totp_state.clone())
                .layer(from_fn_with_state((name, "totp_enroll").into(), user_tracker)),
        )
        .route(
            "/totp/activate",
            post(totp::activate)
                .with_state(totp_state.clone())
                .layer(from_fn_with_state(
                    (name, "totp_activate").into(),
                    user_tracker,
                )),
        )
        .route(
            "/totp/disable",
            post(totp::disable)
                .with_state(totp_state.clone())
                .layer(from_fn_with_state(
                    (name, "totp_disable").into(),
                    user_tracker,
                )),
        )
        .route("/totp/status", get(totp::status).with_state(totp_state))
        // JWT 备选鉴权路径（与现有 Session+Cookie /login 正交）
        // [jwt] 未配置时由 try_global_signer 返回 None → 503
        .route(
            "/login/jwt",
            post(jwt_auth::login_jwt)
                .with_state(jwt_auth_state.clone())
                .layer(from_fn_with_state(
                    (name, "login_jwt").into(),
                    user_tracker,
                )),
        )
        // JWT 登录 2FA 第二步（带频率限制）
        .route(
            "/login/jwt/mfa",
            post(jwt_auth::login_jwt_mfa)
                .with_state(jwt_auth_state.clone())
                .layer(from_fn_with_state(
                    (name, "login_jwt_mfa").into(),
                    user_tracker,
                )),
        )
        .route(
            "/refresh/jwt",
            post(jwt_auth::refresh_jwt)
                .with_state(jwt_auth_state.clone())
                .layer(from_fn_with_state(
                    (name, "refresh_jwt").into(),
                    user_tracker,
                )),
        )
        .route(
            "/logout/jwt",
            delete(jwt_auth::logout_jwt)
                .with_state(jwt_auth_state)
                .layer(from_fn_with_state(
                    (name, "logout_jwt").into(),
                    user_tracker,
                )),
        )
}

/// 本路由模块的 OpenAPI 文档片段（路径相对 `/users` 已在各注解里写全）。
///
/// 聚合 lib.rs 主流程 + email_verify / password_reset / jwt_auth / totp / oauth
/// 各子模块的端点；schema 由 utoipa 从各 path 的 request_body / responses 自动收集。
#[derive(OpenApi)]
#[openapi(
    paths(
        login_token,
        login,
        me,
        refresh_session,
        register,
        logout,
        update_profile,
        login_mfa,
        api_key::create_api_key,
        api_key::list_api_keys,
        api_key::revoke_api_key,
        email_verify::request_verify,
        email_verify::confirm_verify,
        password_reset::request_reset,
        password_reset::confirm_reset,
        jwt_auth::login_jwt,
        jwt_auth::login_jwt_mfa,
        jwt_auth::refresh_jwt,
        jwt_auth::logout_jwt,
        totp::enroll,
        totp::activate,
        totp::disable,
        totp::status,
        oauth_github::start_login,
        oauth_github::callback,
        oauth_google::start_login,
        oauth_google::callback
    ),
    tags((name = "user", description = "用户认证、资料、2FA、OAuth 与 JWT 备选鉴权"))
)]
struct UserApiDoc;

/// 返回 user 路由的 OpenAPI 文档片段，供主 crate 合并进全局文档。
pub fn openapi() -> utoipa::openapi::OpenApi {
    UserApiDoc::openapi()
}

/// 从 HeaderMap 取 User-Agent（缺失 / 非 ASCII 时返回空串）。
/// audit_log 的 truncate 会再做长度截断，这里只负责取值。
pub(crate) fn user_agent_of(headers: &HeaderMap) -> String {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}
