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
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
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
use tibba_middleware::{user_tracker, validate_captcha};
use tibba_model::{Model, ROLE_SUPER_ADMIN, UserModel, UserUpdateParams};
use tibba_model_builtin::RolePermissionModel;
use tibba_session::{Session, SessionResponse, UserSession};
use tibba_util::{
    JsonParams, JsonResult, generate_device_id_cookie, get_device_id_from_cookie, is_development,
    is_test, now, sha256, timestamp, timestamp_hash, uuid, validate_timestamp_hash,
};
use tibba_validator::*;
use validator::Validate;

mod email_verify;
mod oauth_github;
mod oauth_google;
mod password_reset;

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
        };
        err.with_category(ERROR_CATEGORY)
    }
}

/// 登录令牌接口的响应结构体。
#[derive(Serialize)]
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
async fn login_token(State(secret): State<String>) -> JsonResult<LoginTokenResp> {
    let token = uuid();
    let (ts, hash) = timestamp_hash(&token, &secret);

    Ok(Json(LoginTokenResp { ts, hash, token }))
}

/// 登录请求参数，含防重放令牌和用户凭据。
#[derive(Deserialize, Validate, Debug)]
struct LoginParams {
    /// 客户端从 `/login/token` 获取的时间戳
    ts: i64,
    /// 客户端从 `/login/token` 获取的一次性令牌（UUID 格式）
    #[validate(custom(function = "x_uuid"))]
    token: String,
    /// 服务端对 `token` 的签名，用于校验令牌合法性（SHA-256 格式）
    #[validate(custom(function = "x_sha256"))]
    hash: String,
    /// 用户账号
    #[validate(custom(function = "x_user_account"))]
    account: String,
    /// 经过 `sha256(hash:password)` 处理后的密码
    #[validate(custom(function = "x_user_password"))]
    password: String,
}

impl LoginParams {
    /// 校验登录令牌：
    /// - 开发/测试环境下 `ts <= 0` 时跳过校验；
    /// - 时间戳偏差超过 60 秒时拒绝（防重放）；
    /// - 验证 HMAC 签名是否匹配。
    fn validate_token(&self, secret: &str) -> Result<()> {
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
#[derive(Debug, Clone, Serialize, Default)]
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

/// 用户登录接口。
/// 校验令牌合法性后，比对密码（`sha256(hash:stored_password)`），
/// 通过后创建 Session 并返回用户信息。
async fn login(
    State((secret, pool)): State<(String, &'static PgPool)>,
    session: Session,
    JsonParams(params): JsonParams<LoginParams>,
) -> Result<SessionResponse<Json<UserMeResp>>> {
    params.validate_token(&secret)?;
    let account = params.account;
    let Some(user) = UserModel::new().get_by_account(pool, &account).await? else {
        return Err(Error::BadCredentials.into());
    };

    let password = user.password;
    // 密码验证：sha256(hash:存储密码) 须与客户端传入的 password 相等
    let msg = format!("{}:{password}", params.hash);
    if sha256(msg.as_bytes()) != params.password {
        return Err(Error::BadCredentials.into());
    }

    let groups = user.groups.clone().unwrap_or_default();
    let roles = user.roles.clone().unwrap_or_default();

    // 把 roles 翻译为权限码并集，缓存到 Session（避免每次 handler 重查 DB）。
    // 失败仅记录但不阻断登录——RBAC 还未铺开时 role_permissions 可能为空，
    // 登录主流程不应被附属表的 SQL 错误打断。
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

/// 获取当前登录用户信息。
/// 若 Cookie 中无 device_id，则自动生成并写入；
/// 未登录时返回空的 `UserMeResp`。
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
#[derive(Deserialize, Validate)]
struct RegisterParams {
    #[validate(custom(function = "x_user_account"))]
    account: String,
    #[validate(custom(function = "x_user_password"))]
    password: String,
}

/// 注册成功响应体。
#[derive(Serialize)]
struct RegisterResp {
    /// 新用户 ID
    id: u64,
    /// 注册账号
    account: String,
}

/// 注册新用户接口。
/// 第一个注册成功的用户（id=1）自动授予超级管理员角色。
/// 注册成功后若配置了 `on_register` 回调，则异步触发；回调失败不影响注册结果。
async fn register(
    State(state): State<RegisterState>,
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
    Ok(Json(RegisterResp {
        id,
        account: params.account,
    }))
}

/// 刷新 Session 有效期。仅当 Session 满足续期条件时才执行续期操作。
async fn refresh_session(mut session: UserSession) -> Result<Session> {
    if !session.can_renew() {
        return Ok(session.into());
    }
    session.refresh();
    session.save().await?;
    Ok(session.into())
}

/// 登出接口，清除当前 Session。
async fn logout(mut session: Session) -> Session {
    session.reset();
    session
}

/// 更新用户资料的请求参数（所有字段均为可选）。
#[derive(Deserialize, Validate)]
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
async fn update_profile(
    State(pool): State<&'static PgPool>,
    session: UserSession,
    JsonParams(params): JsonParams<UpdateProfileParams>,
) -> Result<StatusCode> {
    let account = session.get_account();
    let params = UserUpdateParams {
        nickname: params.nickname,
        phone: params.phone,
        email: params.email,
        avatar: params.avatar,
        ..Default::default()
    };
    UserModel::new()
        .update_by_account(pool, account, params)
        .await?;
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
                .with_state((params.secret, params.pool))
                .layer(from_fn_with_state(
                    (params.magic_code, params.cache),
                    validate_captcha,
                ))
                .layer(from_fn_with_state((name, "login").into(), user_tracker)),
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
            delete(logout).layer(from_fn_with_state((name, "logout").into(), user_tracker)),
        )
        .route("/profile", patch(update_profile).with_state(params.pool))
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
}
