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

//! JWT 鉴权路由 —— 与现有 `/login` (Session) 路径正交。
//!
//! - `POST /login/jwt`    校验账号密码（复用 `LoginParams`）→ 签 access (HS256) + 发 refresh (opaque)
//! - `POST /refresh/jwt`  用 refresh 取出 user_id，重新查 roles/permissions，签新 access
//! - `DELETE /logout/jwt` 删 Redis 中的 refresh 记录
//!
//! ## Token 生命周期
//! - access：JWT，含 `sub/account/roles/permissions/exp/jti`，签发后 **无状态**，
//!   验签即放行；TTL = `[jwt] access_ttl`（默认 15min）
//! - refresh：opaque UUID，**不是 JWT**，存 Redis `jwt_refresh:{token} → (user_id, account)`，
//!   TTL = `[jwt] refresh_ttl`（默认 7d）。logout 即删，达成"撤销"语义

use crate::{LoginParams, totp, user_agent_of};
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, Snafu};
use sqlx::PgPool;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_jwt::JwtSigner;
use tibba_middleware::{ClientIp, RequestId};
use tibba_model::{Model, User, UserModel};
use tibba_model_builtin::{AuditLogModel, AuditLogParams, RolePermissionModel};
use tibba_util::{JsonParams, JsonResult, sha256, uuid};
use tibba_validator::x_uuid;
use tracing::warn;
use utoipa::ToSchema;
use validator::Validate;

const REFRESH_REDIS_PREFIX: &str = "jwt_refresh:";
const LOG_TARGET: &str = "tibba:jwt_auth";

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// `[jwt]` 未配置 secret，端点统一 503
    #[snafu(display("jwt is not enabled (set [jwt] secret)"))]
    NotEnabled,
    /// refresh token 不在 Redis 或已过期 → 401
    #[snafu(display("refresh token invalid or expired"))]
    InvalidRefresh,
    /// refresh 解出的本地 user 已被软删除 → 401
    #[snafu(display("user no longer exists"))]
    UserGone,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::NotEnabled => BaseError::new("jwt is not enabled (set [jwt] secret)")
                .with_sub_category("not_enabled")
                .with_status(503)
                .with_exception(true),
            Error::InvalidRefresh => BaseError::new("refresh token invalid or expired")
                .with_sub_category("invalid_refresh")
                .with_status(401)
                .with_exception(false),
            Error::UserGone => BaseError::new("user no longer exists")
                .with_sub_category("user_gone")
                .with_status(401)
                .with_exception(false),
        };
        err.with_category("jwt_auth")
    }
}

#[derive(Clone)]
pub(crate) struct JwtAuthState {
    pub pool: &'static PgPool,
    pub cache: &'static RedisCache,
    /// /login 用的签名 secret（与表单登录共用，保证防重放校验一致）
    pub secret: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct JwtLoginResp {
    /// HS256 access JWT
    pub access_token: String,
    /// opaque refresh token（UUID 字符串）
    pub refresh_token: String,
    pub token_type: &'static str,
    /// access TTL（秒），方便客户端排程刷新
    pub expires_in: u64,
}

/// 2FA 登录挑战响应（JWT 路径）：密码已过但需第二步时返回，**不**签发 token。
#[derive(Serialize, ToSchema)]
struct JwtChallenge {
    /// 恒为 `true`，客户端据此进入第二步（调用 `/login/jwt/mfa`）。
    mfa_required: bool,
    /// 一次性挑战令牌（5 分钟有效）。
    mfa_token: String,
}

/// `POST /login/jwt` —— 同 `/login` 校验链。已启用 2FA 时返回 [`JwtChallenge`]
/// 而不签发 token；否则签 access + 发 refresh。
#[utoipa::path(
    post,
    path = "/users/login/jwt",
    tag = "user",
    request_body = LoginParams,
    responses(
        (status = 200, description = "签发 access + refresh（JwtLoginResp）；已启用 2FA 时返回 { mfa_required, mfa_token }"),
        (status = 401, description = "账号或密码错误"),
        (status = 503, description = "[jwt] 未配置 secret，JWT 鉴权未启用")
    )
)]
pub(crate) async fn login_jwt(
    State(state): State<JwtAuthState>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<LoginParams>,
) -> Result<Response, BaseError> {
    let signer = tibba_jwt::try_global_signer().context(NotEnabledSnafu)?;

    // 1. 防重放令牌校验（与表单 /login 一致）
    params.validate_token(&state.secret)?;

    // 2. 账号 + 密码（hash:stored_password 的 sha256 比对）
    let account = params.account.clone();
    let ip_str = ip.to_string();

    // 暴力破解闸门：与表单 /login 同策略，防攻击者改走 JWT 路径绕过锁定
    crate::login_guard::ensure_not_locked(state.cache, &account, &ip_str).await?;

    let Some(user) = UserModel::new().get_by_account(state.pool, &account).await? else {
        crate::login_guard::record_failure(state.cache, &account, &ip_str).await;
        return Err(crate::Error::BadCredentials.into());
    };
    let msg = format!("{}:{}", params.hash, user.password);
    if sha256(msg.as_bytes()) != params.password {
        crate::login_guard::record_failure(state.cache, &account, &ip_str).await;
        return Err(crate::Error::BadCredentials.into());
    }

    // 凭证正确：清账号失败计数
    crate::login_guard::clear_failures(state.cache, &account).await;

    // 3. 2FA 闸门：已启用则不签 token，签发挑战令牌（与 Session 路径前缀隔离）
    let totp_state = UserModel::new().get_totp_state(state.pool, user.id).await?;
    if totp_state.enabled {
        let mfa_token =
            totp::create_mfa_challenge(state.cache, totp::MFA_PREFIX_JWT, user.id).await?;
        return Ok(Json(JwtChallenge {
            mfa_required: true,
            mfa_token,
        })
        .into_response());
    }

    let resp = issue_jwt(
        signer,
        &state,
        user,
        &request_id,
        ip.to_string(),
        &headers,
        "user.login_jwt",
    )
    .await?;
    Ok(Json(resp).into_response())
}

/// 角色→权限、签 access、发 refresh、last_login + audit 的公共尾段。
/// `login_jwt` 与 `login_jwt_mfa` 共用，避免 token 签发逻辑两处漂移。
async fn issue_jwt(
    signer: &JwtSigner,
    state: &JwtAuthState,
    user: User,
    request_id: &RequestId,
    ip: String,
    headers: &HeaderMap,
    action: &'static str,
) -> Result<JwtLoginResp, BaseError> {
    // 角色 → 权限并集（与 session login 一致）
    let roles = user.roles.clone().unwrap_or_default();
    let permissions = RolePermissionModel::new()
        .list_permissions_for_roles(state.pool, &roles)
        .await
        .unwrap_or_default();

    // 签 access JWT
    let access_token = signer
        .sign_access(user.id, &user.account, roles, permissions)
        .map_err(BaseError::from)?;

    // 发 opaque refresh，Redis 持有 (user_id, account)
    let refresh_token = uuid();
    state
        .cache
        .set_struct(
            &format!("{REFRESH_REDIS_PREFIX}{refresh_token}"),
            &(user.id, user.account.clone()),
            Some(signer.refresh_ttl()),
        )
        .await?;

    // last_login_at + audit（与 session login 平行；失败仅日志，不阻塞）
    if let Err(e) = UserModel::new()
        .update_last_login_at(state.pool, &user.account)
        .await
    {
        warn!(target: LOG_TARGET, error = %e, "update_last_login_at failed");
    }
    let _ = AuditLogModel::new()
        .log(
            state.pool,
            AuditLogParams::new(action)
                .with_user(user.id)
                .with_target("user", user.id.to_string())
                .with_request(request_id.as_str(), ip, user_agent_of(headers)),
        )
        .await;

    Ok(JwtLoginResp {
        access_token,
        refresh_token,
        token_type: "Bearer",
        expires_in: signer.access_ttl().as_secs(),
    })
}

/// 第二步登录参数（JWT 路径）：挑战令牌 + 动态码/恢复码。
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub(crate) struct JwtMfaParams {
    /// `/login/jwt` 返回的一次性挑战令牌（UUID）
    #[validate(custom(function = "x_uuid"))]
    pub mfa_token: String,
    /// TOTP 动态码（6 位）或恢复码（`xxxxx-xxxxx`）
    #[validate(length(min = 6, max = 32))]
    pub code: String,
}

/// `POST /login/jwt/mfa` —— 完成 JWT 登录第二步。
/// 挑战令牌一次性：无论校验成败都已消费，验证失败需重新 `/login/jwt`。
#[utoipa::path(
    post,
    path = "/users/login/jwt/mfa",
    tag = "user",
    request_body = JwtMfaParams,
    responses(
        (status = 200, description = "二步通过，签发 access + refresh", body = JwtLoginResp),
        (status = 401, description = "挑战令牌失效或动态码/恢复码错误")
    )
)]
pub(crate) async fn login_jwt_mfa(
    State(state): State<JwtAuthState>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<JwtMfaParams>,
) -> JsonResult<JwtLoginResp> {
    let signer = tibba_jwt::try_global_signer().context(NotEnabledSnafu)?;

    let user_id = totp::consume_mfa_challenge(state.cache, totp::MFA_PREFIX_JWT, &params.mfa_token)
        .await?
        .ok_or(totp::Error::InvalidChallenge)?;

    if !totp::verify_second_factor(state.pool, &state.secret, user_id, params.code.trim()).await? {
        return Err(totp::Error::BadCode.into());
    }

    let user = UserModel::new()
        .get_by_id(state.pool, user_id as u64)
        .await?
        .context(UserGoneSnafu)?;

    let resp = issue_jwt(
        signer,
        &state,
        user,
        &request_id,
        ip.to_string(),
        &headers,
        "user.login_jwt_mfa",
    )
    .await?;
    Ok(Json(resp))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub(crate) struct RefreshParams {
    /// opaque UUID（与登录响应里的 refresh_token 同形）
    #[validate(custom(function = "x_uuid"))]
    pub refresh_token: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RefreshResp {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
}

/// `POST /refresh/jwt` —— 用 refresh 重签 access。refresh 不旋转（简单实现），
/// 想要 rotation 的安全态可后续扩展。
#[utoipa::path(
    post,
    path = "/users/refresh/jwt",
    tag = "user",
    request_body = RefreshParams,
    responses(
        (status = 200, description = "重签 access 成功", body = RefreshResp),
        (status = 401, description = "refresh token 无效或已过期")
    )
)]
pub(crate) async fn refresh_jwt(
    State(state): State<JwtAuthState>,
    JsonParams(params): JsonParams<RefreshParams>,
) -> JsonResult<RefreshResp> {
    let signer = tibba_jwt::try_global_signer().context(NotEnabledSnafu)?;

    let key = format!("{REFRESH_REDIS_PREFIX}{}", params.refresh_token);
    let stored: Option<(i64, String)> = state.cache.get_struct(&key).await?;
    let (user_id, account) = stored.context(InvalidRefreshSnafu)?;

    // refresh 命中但本地 user 已软删 → 视为 401
    let user = UserModel::new()
        .get_by_id(state.pool, user_id as u64)
        .await?
        .context(UserGoneSnafu)?;

    let roles = user.roles.clone().unwrap_or_default();
    let permissions = RolePermissionModel::new()
        .list_permissions_for_roles(state.pool, &roles)
        .await
        .unwrap_or_default();

    let access_token = signer
        .sign_access(user_id, &account, roles, permissions)
        .map_err(BaseError::from)?;

    Ok(Json(RefreshResp {
        access_token,
        token_type: "Bearer",
        expires_in: signer.access_ttl().as_secs(),
    }))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub(crate) struct LogoutParams {
    #[validate(custom(function = "x_uuid"))]
    pub refresh_token: String,
}

/// `DELETE /logout/jwt` —— 删除 refresh，使后续无法续签 access。
/// access 在自身 exp 之前仍可用——可接受的折衷（短 TTL 限制窗口）。
#[utoipa::path(
    delete,
    path = "/users/logout/jwt",
    tag = "user",
    request_body = LogoutParams,
    responses((status = 204, description = "refresh 已删除，无法再续签"))
)]
pub(crate) async fn logout_jwt(
    State(state): State<JwtAuthState>,
    JsonParams(params): JsonParams<LogoutParams>,
) -> Result<StatusCode, BaseError> {
    let key = format!("{REFRESH_REDIS_PREFIX}{}", params.refresh_token);
    if let Err(e) = state.cache.del(&key).await {
        warn!(target: LOG_TARGET, error = %e, "delete refresh token failed");
    }
    Ok(StatusCode::NO_CONTENT)
}
