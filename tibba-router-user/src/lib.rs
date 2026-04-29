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

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tibba_cache::RedisCache;
use tibba_error::Error;
use tibba_middleware::{user_tracker, validate_captcha};
use tibba_model::{Model, ROLE_SUPER_ADMIN, UserModel, UserUpdateParams};
use tibba_session::{Session, SessionResponse, UserSession};
use tibba_util::{
    JsonParams, JsonResult, generate_device_id_cookie, get_device_id_from_cookie, is_development,
    is_test, now, sha256, timestamp, timestamp_hash, uuid, validate_timestamp_hash,
};
use tibba_validator::*;
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

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
            return Err(Error::new("timestamp is expired"));
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
    let account_password_err = Error::new("account or password is wrong");
    let Some(user) = UserModel::new().get_by_account(pool, &account).await? else {
        return Err(account_password_err);
    };

    let password = user.password;
    // 密码验证：sha256(hash:存储密码) 须与客户端传入的 password 相等
    let msg = format!("{}:{password}", params.hash);
    if sha256(msg.as_bytes()) != params.password {
        return Err(account_password_err);
    }

    let groups = user.groups.clone().unwrap_or_default();
    let roles = user.roles.clone().unwrap_or_default();

    let session = session
        .with_account(&account, user.id)
        .with_groups(groups)
        .with_roles(roles);
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
        .ok_or(Error::new("user not found"))?;
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
async fn register(
    State(pool): State<&'static PgPool>,
    JsonParams(params): JsonParams<RegisterParams>,
) -> JsonResult<RegisterResp> {
    let model = UserModel::new();
    let id = model
        .register(pool, &params.account, &params.password)
        .await?;
    // 首个用户自动升级为超级管理员
    if id == 1 {
        model
            .update_by_id(pool, id, json!({ "roles": [ROLE_SUPER_ADMIN] }))
            .await?;
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
    /// Redis 缓存（存储验证码答案）
    pub cache: &'static RedisCache,
}

/// 创建用户相关路由，包含以下端点：
/// - `GET  /login/token` — 获取登录令牌（带频率限制中间件）
/// - `POST /login`       — 用户登录（带验证码校验 + 频率限制中间件）
/// - `GET  /me`          — 获取当前用户信息
/// - `PATCH /refresh`    — 刷新 Session 有效期
/// - `POST /register`    — 注册新用户（带频率限制中间件）
/// - `DELETE /logout`    — 登出（带频率限制中间件）
/// - `PATCH /profile`    — 更新个人资料
pub fn new_user_router(params: UserRouterParams) -> Router {
    let name = "user";

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
                .with_state(params.pool)
                .layer(from_fn_with_state((name, "register").into(), user_tracker)),
        )
        .route(
            "/logout",
            delete(logout).layer(from_fn_with_state((name, "logout").into(), user_tracker)),
        )
        .route("/profile", patch(update_profile).with_state(params.pool))
}
