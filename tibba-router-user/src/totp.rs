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

//! TOTP 两步验证（2FA）路由与登录挑战支撑。
//!
//! ## 端点（均需登录态）
//! - `POST /totp/enroll`   — 生成密钥，返回 base32 + `otpauth://` URI（待激活）
//! - `POST /totp/activate` — 提交动态码确认，激活 2FA，返回一次性恢复码
//! - `POST /totp/disable`  — 提交动态码/恢复码关闭 2FA
//! - `GET  /totp/status`   — 返回 `{ enrolled, enabled }`
//!
//! ## 登录挑战（供 lib.rs / jwt_auth.rs 复用）
//! 密码校验通过后，若用户已启用 2FA，则不直接建立会话，而是签发短期
//! `mfa_token`（Redis，5min），客户端凭它 + 动态码调 `/login/mfa`
//! （或 `/login/jwt/mfa`）完成第二步。见 [`create_mfa_challenge`] /
//! [`consume_mfa_challenge`] / [`verify_second_factor`]。

use crate::user_agent_of;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, Snafu};
use sqlx::PgPool;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_model::{Model, UserModel};
use tibba_model_builtin::{AuditLogModel, AuditLogParams};
use tibba_session::UserSession;
use tibba_totp::SecretCipher;
use tibba_util::{JsonParams, JsonResult, timestamp, uuid};
use utoipa::ToSchema;
use validator::Validate;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "totp_router";

/// `otpauth://` URI 的 issuer，显示在 authenticator app 中作为账号分组名。
const ISSUER: &str = "tibba";

/// 恢复码数量：激活时一次性生成，明文仅返回一次。
const RECOVERY_CODE_COUNT: usize = 10;

/// 登录挑战令牌有效期：5 分钟，足够用户切到 authenticator app 取码。
pub(crate) const MFA_TTL: Duration = Duration::from_secs(5 * 60);
/// Session 登录挑战的 Redis 前缀。
pub(crate) const MFA_PREFIX_SESSION: &str = "mfa_login:";
/// JWT 登录挑战的 Redis 前缀（与 Session 隔离，令牌不可跨路径复用）。
pub(crate) const MFA_PREFIX_JWT: &str = "mfa_login_jwt:";

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// 已启用 2FA 时重复 enroll（HTTP 409）
    #[snafu(display("2fa is already enabled"))]
    AlreadyEnabled,
    /// 未先 enroll 就 activate（HTTP 400）
    #[snafu(display("no pending 2fa enrollment"))]
    NotPending,
    /// 未启用 2FA 时调用 disable / 完成挑战（HTTP 400）
    #[snafu(display("2fa is not enabled"))]
    NotEnabled,
    /// 动态码或恢复码错误（HTTP 401）
    #[snafu(display("invalid 2fa code"))]
    BadCode,
    /// 登录挑战令牌不存在或已过期（HTTP 401）
    #[snafu(display("mfa challenge invalid or expired"))]
    InvalidChallenge,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::AlreadyEnabled => BaseError::new("2fa is already enabled")
                .with_sub_category("already_enabled")
                .with_status(409)
                .with_exception(false),
            Error::NotPending => BaseError::new("no pending 2fa enrollment")
                .with_sub_category("not_pending")
                .with_status(400)
                .with_exception(false),
            Error::NotEnabled => BaseError::new("2fa is not enabled")
                .with_sub_category("not_enabled")
                .with_status(400)
                .with_exception(false),
            Error::BadCode => BaseError::new("invalid 2fa code")
                .with_sub_category("bad_code")
                .with_status(401)
                .with_exception(false),
            Error::InvalidChallenge => BaseError::new("mfa challenge invalid or expired")
                .with_sub_category("invalid_challenge")
                .with_status(401)
                .with_exception(false),
        };
        err.with_category(ERROR_CATEGORY)
    }
}

/// TOTP 端点共用 State：DB + 应用 secret（派生加密密钥）。
#[derive(Clone)]
pub(crate) struct TotpRouterState {
    pub pool: &'static PgPool,
    pub secret: String,
}

/// `POST /totp/enroll` 响应：base32 密钥 + 可扫描的 `otpauth://` URI。
#[derive(Serialize, ToSchema)]
pub(crate) struct EnrollResp {
    /// base32 编码密钥，供用户手动输入 authenticator app
    secret: String,
    /// `otpauth://totp/...` URI，前端据此渲染二维码
    otpauth_uri: String,
}

/// 生成新密钥并以「待激活」态落库（覆盖任何旧的未激活密钥）。
/// 已启用 2FA 时拒绝，必须先 disable 再重新 enroll，避免误覆盖在用密钥。
#[utoipa::path(
    post,
    path = "/users/totp/enroll",
    tag = "user",
    responses(
        (status = 200, description = "返回 base32 密钥与 otpauth URI（待激活）", body = EnrollResp),
        (status = 409, description = "2FA 已启用，需先 disable")
    )
)]
pub(crate) async fn enroll(
    State(state): State<TotpRouterState>,
    session: UserSession,
) -> JsonResult<EnrollResp> {
    let user_id = session.get_user_id();
    let account = session.get_account().to_string();

    let current = UserModel::new().get_totp_state(state.pool, user_id).await?;
    if current.enabled {
        return Err(Error::AlreadyEnabled.into());
    }

    let secret = tibba_totp::generate_secret();
    let cipher = SecretCipher::from_app_secret(&state.secret);
    let secret_cipher = cipher.encrypt(&secret)?;
    UserModel::new()
        .set_totp_pending(state.pool, user_id, &secret_cipher)
        .await?;

    let secret_b32 = tibba_totp::base32_encode(&secret);
    let otpauth_uri = tibba_totp::otpauth_uri(&secret_b32, &account, ISSUER);
    Ok(Json(EnrollResp {
        secret: secret_b32,
        otpauth_uri,
    }))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub(crate) struct CodeParams {
    /// 动态码（6 位）或恢复码（`xxxxx-xxxxx`）
    #[validate(length(min = 6, max = 32))]
    pub code: String,
}

/// `POST /totp/activate` 响应：一次性恢复码（明文仅此一次返回）。
#[derive(Serialize, ToSchema)]
pub(crate) struct ActivateResp {
    /// 请用户立即妥善保存；服务端只存哈希，遗失无法找回
    recovery_codes: Vec<String>,
}

/// 用动态码确认待激活密钥，激活 2FA 并下发恢复码。
#[utoipa::path(
    post,
    path = "/users/totp/activate",
    tag = "user",
    request_body = CodeParams,
    responses(
        (status = 200, description = "激活成功，返回一次性恢复码（仅此一次）", body = ActivateResp),
        (status = 400, description = "无待激活密钥"),
        (status = 401, description = "动态码错误")
    )
)]
pub(crate) async fn activate(
    State(state): State<TotpRouterState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<CodeParams>,
) -> JsonResult<ActivateResp> {
    let user_id = session.get_user_id();
    let totp = UserModel::new().get_totp_state(state.pool, user_id).await?;
    if totp.enabled {
        return Err(Error::AlreadyEnabled.into());
    }
    let secret_cipher = totp.secret_cipher.as_deref().context(NotPendingSnafu)?;

    // 仅接受 TOTP 动态码激活（此时尚无恢复码）
    let cipher = SecretCipher::from_app_secret(&state.secret);
    let secret = cipher.decrypt(secret_cipher)?;
    if !tibba_totp::verify_code(&secret, params.code.trim(), timestamp()) {
        return Err(Error::BadCode.into());
    }

    // 生成恢复码：明文返回用户，仅哈希落库
    let recovery_codes = tibba_totp::generate_recovery_codes(RECOVERY_CODE_COUNT);
    let hashes: Vec<String> = recovery_codes
        .iter()
        .map(|c| tibba_totp::hash_recovery_code(c))
        .collect();
    UserModel::new()
        .activate_totp(state.pool, user_id, &hashes)
        .await?;

    let _ = AuditLogModel::new()
        .log(
            state.pool,
            AuditLogParams::new("user.totp_enable")
                .with_user(user_id)
                .with_target("user", user_id.to_string())
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(&headers)),
        )
        .await;

    Ok(Json(ActivateResp { recovery_codes }))
}

/// `POST /totp/disable` —— 提交动态码或恢复码关闭 2FA。
#[utoipa::path(
    post,
    path = "/users/totp/disable",
    tag = "user",
    request_body = CodeParams,
    responses(
        (status = 204, description = "2FA 已关闭"),
        (status = 400, description = "2FA 未启用"),
        (status = 401, description = "动态码/恢复码错误")
    )
)]
pub(crate) async fn disable(
    State(state): State<TotpRouterState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<CodeParams>,
) -> Result<StatusCode> {
    let user_id = session.get_user_id();
    let totp = UserModel::new().get_totp_state(state.pool, user_id).await?;
    if !totp.enabled {
        return Err(Error::NotEnabled.into());
    }

    if !verify_second_factor(state.pool, &state.secret, user_id, params.code.trim()).await? {
        return Err(Error::BadCode.into());
    }

    UserModel::new().disable_totp(state.pool, user_id).await?;

    let _ = AuditLogModel::new()
        .log(
            state.pool,
            AuditLogParams::new("user.totp_disable")
                .with_user(user_id)
                .with_target("user", user_id.to_string())
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(&headers)),
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /totp/status` 响应。
#[derive(Serialize, ToSchema)]
pub(crate) struct StatusResp {
    /// 是否已生成密钥（含待激活）
    enrolled: bool,
    /// 是否已激活并在登录时强制
    enabled: bool,
}

/// 返回当前用户的 2FA 状态。
#[utoipa::path(
    get,
    path = "/users/totp/status",
    tag = "user",
    responses((status = 200, description = "2FA 状态 { enrolled, enabled }", body = StatusResp))
)]
pub(crate) async fn status(
    State(state): State<TotpRouterState>,
    session: UserSession,
) -> JsonResult<StatusResp> {
    let user_id = session.get_user_id();
    let totp = UserModel::new().get_totp_state(state.pool, user_id).await?;
    Ok(Json(StatusResp {
        enrolled: totp.secret_cipher.is_some(),
        enabled: totp.enabled,
    }))
}

/// 校验第二因子：先按 TOTP 动态码验，失败再尝试作为一次性恢复码消费。
///
/// 恢复码命中时通过 `consume_recovery_code` 原子移除，杜绝重放。
/// 6 位动态码不会误命中恢复码哈希，反之亦然，故两路顺序尝试无副作用。
pub(crate) async fn verify_second_factor(
    pool: &PgPool,
    app_secret: &str,
    user_id: i64,
    code: &str,
) -> Result<bool> {
    let totp = UserModel::new().get_totp_state(pool, user_id).await?;
    // 1) TOTP 动态码
    if let Some(secret_cipher) = &totp.secret_cipher {
        let cipher = SecretCipher::from_app_secret(app_secret);
        let secret = cipher.decrypt(secret_cipher)?;
        if tibba_totp::verify_code(&secret, code, timestamp()) {
            return Ok(true);
        }
    }
    // 2) 恢复码（原子消费）
    let hash = tibba_totp::hash_recovery_code(code);
    if totp.recovery_hashes.contains(&hash) {
        let consumed = UserModel::new()
            .consume_recovery_code(pool, user_id, &hash)
            .await?;
        return Ok(consumed);
    }
    Ok(false)
}

/// 创建登录挑战：把 `user_id` 以短期令牌存入 Redis，返回令牌交给客户端。
pub(crate) async fn create_mfa_challenge(
    cache: &RedisCache,
    prefix: &str,
    user_id: i64,
) -> Result<String> {
    let token = uuid();
    cache
        .set_struct(&format!("{prefix}{token}"), &user_id, Some(MFA_TTL))
        .await?;
    Ok(token)
}

/// 消费登录挑战：命中即删除并返回 `user_id`，未命中返回 `None`。
pub(crate) async fn consume_mfa_challenge(
    cache: &RedisCache,
    prefix: &str,
    token: &str,
) -> Result<Option<i64>> {
    let key = format!("{prefix}{token}");
    let user_id: Option<i64> = cache.get_struct(&key).await?;
    if user_id.is_some() {
        // 一次性：命中后立即删，失败不影响主流程（令牌 5min 内自然过期）
        let _ = cache.del(&key).await;
    }
    Ok(user_id)
}
