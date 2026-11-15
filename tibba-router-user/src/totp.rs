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
//! ## 登录挑战（供 lib.rs / jwt_auth.rs / oauth 复用）
//! 密码或 OAuth 校验通过后，若用户已启用 2FA，则不直接建立会话，而是签发短期
//! `mfa_token`（Redis，120s），客户端凭它 + 动态码调 `/login/mfa`
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
// 本文件多处用 `secret_cipher` 作局部变量（对应数据库列名），导入改名避免遮蔽
use tibba_totp::secret_cipher as totp_cipher;
use tibba_util::{JsonParams, JsonResult, timestamp, uuid};
use tracing::warn;
use utoipa::ToSchema;
use validator::Validate;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "totp_router";
const LOG_TARGET: &str = "tibba:totp_router";

/// `otpauth://` URI 的 issuer，显示在 authenticator app 中作为账号分组名。
const ISSUER: &str = "tibba";

/// 恢复码数量：激活时一次性生成，明文仅返回一次。
const RECOVERY_CODE_COUNT: usize = 10;

/// 登录挑战令牌有效期：120 秒，缩短中间挑战暴露窗口。
pub(crate) const MFA_TTL: Duration = Duration::from_secs(120);
/// Session 登录挑战的 Redis 前缀。
pub(crate) const MFA_PREFIX_SESSION: &str = "mfa_login:";
/// JWT 登录挑战的 Redis 前缀（与 Session 隔离，令牌不可跨路径复用）。
pub(crate) const MFA_PREFIX_JWT: &str = "mfa_login_jwt:";
/// TOTP 已消费 step 的标记 TTL：覆盖动态码有效窗（±SKEW × PERIOD）并留余量，到期自动清理。
const TOTP_REPLAY_TTL: Duration = Duration::from_secs(120);
/// 单用户 2FA 失败计数键前缀。
const TOTP_FAIL_PREFIX: &str = "totp:fail:";
/// 单用户 2FA 失败阈值：窗口内失败达阈值即锁定。
const TOTP_FAIL_MAX: i64 = 5;
/// 2FA 失败计数固定窗口。
const TOTP_FAIL_WINDOW: Duration = Duration::from_secs(15 * 60);

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
    /// 2FA 验证失败次数过多，账户被临时锁定（HTTP 429）
    #[snafu(display("too many failed 2fa attempts, please retry later"))]
    TooManyAttempts,
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
            Error::TooManyAttempts => {
                BaseError::new("too many failed 2fa attempts, please retry later")
                    .with_sub_category("too_many_attempts")
                    .with_status(429)
                    .with_exception(false)
            }
        };
        err.with_category(ERROR_CATEGORY)
    }
}

/// TOTP 端点共用 State：DB + 应用 secret（派生加密密钥）。
#[derive(Clone)]
pub(crate) struct TotpRouterState {
    pub pool: &'static PgPool,
    pub cache: &'static RedisCache,
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
    let cipher = totp_cipher(&state.secret);
    let secret_cipher = tibba_totp::encrypt_for_user(&cipher, user_id, &secret)?;
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
    ensure_totp_not_locked(state.cache, user_id).await?;

    let totp = UserModel::new().get_totp_state(state.pool, user_id).await?;
    if totp.enabled {
        return Err(Error::AlreadyEnabled.into());
    }
    let secret_cipher = totp.secret_cipher.as_deref().context(NotPendingSnafu)?;

    // 仅接受 TOTP 动态码激活（此时尚无恢复码），并纳入防重放与失败锁定
    let cipher = totp_cipher(&state.secret);
    let secret = tibba_totp::decrypt_for_user(&cipher, user_id, secret_cipher)?;
    if !verify_and_mark_totp_step(state.cache, user_id, &secret, params.code.trim()).await? {
        record_totp_failure(state.cache, user_id).await;
        return Err(Error::BadCode.into());
    }
    clear_totp_failures(state.cache, user_id).await;

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

    if !verify_second_factor(
        state.pool,
        state.cache,
        &state.secret,
        user_id,
        params.code.trim(),
    )
    .await?
    {
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

fn fail_key(user_id: i64) -> String {
    format!("{TOTP_FAIL_PREFIX}{user_id}")
}

/// 检查用户在当前窗口内的 2FA 失败次数是否超限。
async fn ensure_totp_not_locked(cache: &RedisCache, user_id: i64) -> Result<()> {
    let count = cache.get::<i64>(&fail_key(user_id)).await?;
    if count.unwrap_or(0) >= TOTP_FAIL_MAX {
        return Err(Error::TooManyAttempts.into());
    }
    Ok(())
}

/// 记录一次 2FA 失败；Redis 故障仅记录告警，避免把正常 401 升级成 500。
async fn record_totp_failure(cache: &RedisCache, user_id: i64) {
    if let Err(e) = cache
        .incr(&fail_key(user_id), 1, Some(TOTP_FAIL_WINDOW))
        .await
    {
        warn!(target: LOG_TARGET, user_id, error = %e, "record totp failure failed");
    }
}

/// 2FA 验证通过后清除失败计数。
async fn clear_totp_failures(cache: &RedisCache, user_id: i64) {
    if let Err(e) = cache.del(&fail_key(user_id)).await {
        warn!(target: LOG_TARGET, user_id, error = %e, "clear totp failures failed");
    }
}

/// 校验 TOTP 动态码并原子标记同一时间步防重放。
async fn verify_and_mark_totp_step(
    cache: &RedisCache,
    user_id: i64,
    secret: &[u8],
    code: &str,
) -> Result<bool> {
    let Some(counter) = tibba_totp::verify_code_step(secret, code, timestamp()) else {
        return Ok(false);
    };
    // 防重放：同一 step 只接受一次。incr 原子自增并在首次设 TTL（120s），
    // count == 1 为首次使用（接受），> 1 说明该 step 已用过（重放，拒绝）。
    let used_key = format!("totp:used:{user_id}:{counter}");
    let count = cache.incr(&used_key, 1, Some(TOTP_REPLAY_TTL)).await?;
    Ok(count == 1)
}

/// 校验第二因子：先按 TOTP 动态码验，失败再尝试作为一次性恢复码消费。
///
/// 恢复码命中时通过常量时间全量扫描定位并通过 `consume_recovery_code` 原子移除，杜绝重放与计时侧信道。
pub(crate) async fn verify_second_factor(
    pool: &PgPool,
    cache: &RedisCache,
    app_secret: &str,
    user_id: i64,
    code: &str,
) -> Result<bool> {
    ensure_totp_not_locked(cache, user_id).await?;

    let totp = UserModel::new().get_totp_state(pool, user_id).await?;
    // 1) TOTP 动态码
    if let Some(secret_cipher) = &totp.secret_cipher {
        let cipher = totp_cipher(app_secret);
        let secret = tibba_totp::decrypt_for_user(&cipher, user_id, secret_cipher)?;
        if verify_and_mark_totp_step(cache, user_id, &secret, code).await? {
            clear_totp_failures(cache, user_id).await;
            return Ok(true);
        }
    }
    // 2) 恢复码（全数组常量时间匹配 + 原子消费）
    if let Some(idx) = tibba_totp::match_recovery_code(&totp.recovery_hashes, code)
        && let Some(matched_hash) = totp.recovery_hashes.get(idx)
    {
        let consumed = UserModel::new()
            .consume_recovery_code(pool, user_id, matched_hash)
            .await?;
        if consumed {
            clear_totp_failures(cache, user_id).await;
            return Ok(true);
        }
    }
    record_totp_failure(cache, user_id).await;
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
    cache.get_del_struct(&key).await.map_err(Into::into)
}
