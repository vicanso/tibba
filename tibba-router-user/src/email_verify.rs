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

//! 邮箱验证流程：
//! - `POST /email/verify/request` — 登录态触发，发验证码邮件到用户绑定邮箱
//! - `POST /email/verify/confirm` — 提交 token，写入 `users.email_verified_at = NOW()`
//!
//! Token 用 UUID（36 字符），存 Redis 24h，单次使用后立即 del。

use crate::user_agent_of;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, Snafu};
use sqlx::PgPool;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_email::EmailConfig;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_model::{Model, UserModel};
use tibba_model_builtin::{AuditLogModel, AuditLogParams};
use tibba_session::UserSession;
use tibba_util::{JsonParams, uuid, x_uuid};
use utoipa::ToSchema;
use validator::Validate;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "email_verify";
const REDIS_PREFIX: &str = "email_verify:";
const TOKEN_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// 用户没有绑定邮箱，无法发送验证邮件（HTTP 400）
    #[snafu(display("no email bound to this account"))]
    NoEmailOnAccount,
    /// Token 不存在或已过期（HTTP 401）
    #[snafu(display("invalid or expired token"))]
    InvalidToken,
    /// 申请验证之后邮箱被改过：验证码对应的地址已不是账号当前邮箱（HTTP 409）
    #[snafu(display("email changed since verification was requested"))]
    EmailChanged,
}

/// 验证码在缓存中对应的内容：**用户与验证码实际发往的邮箱**。
///
/// 此前只存 user_id，确认时直接把「当前邮箱」标成已验证——于是先对自己的
/// 邮箱申请、再把资料邮箱改成他人的、最后用自己收到的验证码确认，他人的邮箱
/// 就成了「已验证」。把邮箱一并存下，确认时要求账号当前邮箱与之一致。
#[derive(Debug, Serialize, Deserialize)]
struct PendingVerification {
    user_id: i64,
    email: String,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::NoEmailOnAccount => BaseError::new("no email bound to this account")
                .with_sub_category("no_email")
                .with_status(400)
                .with_exception(false),
            Error::InvalidToken => BaseError::new("invalid or expired token")
                .with_sub_category("invalid_token")
                .with_status(401)
                .with_exception(false),
            Error::EmailChanged => BaseError::new("email changed since verification was requested")
                .with_sub_category("email_changed")
                .with_status(409),
        };
        err.with_category(ERROR_CATEGORY)
    }
}

/// 邮箱验证 handler 共用 State：DB / Redis / 邮件配置三件套。
#[derive(Clone)]
pub(crate) struct EmailVerifyState {
    pub pool: &'static PgPool,
    pub cache: &'static RedisCache,
    pub email_config: &'static EmailConfig,
}

/// 触发：登录态调用，向用户绑定邮箱发送验证码。
#[utoipa::path(
    post,
    path = "/users/email/verify/request",
    tag = "user",
    responses(
        (status = 204, description = "验证邮件已发送"),
        (status = 400, description = "账号未绑定邮箱")
    )
)]
pub(crate) async fn request_verify(
    State(state): State<EmailVerifyState>,
    session: UserSession,
) -> Result<StatusCode> {
    let user = UserModel::new()
        .get_by_account(state.pool, session.get_account())
        .await?
        .ok_or_else(|| {
            BaseError::new("user not found")
                .with_category(ERROR_CATEGORY)
                .with_status(401)
                .with_exception(false)
        })?;

    let email = user
        .email
        .as_deref()
        .filter(|s| !s.is_empty())
        .context(NoEmailOnAccountSnafu)?;

    let token = uuid();
    state
        .cache
        .set_struct(
            &format!("{REDIS_PREFIX}{token}"),
            &PendingVerification {
                user_id: user.id,
                email: email.to_string(),
            },
            Some(Duration::from_secs(TOKEN_TTL_SECS)),
        )
        .await?;

    let subject = "验证您的邮箱";
    let body = format!(
        "您好，\n\n\
         请使用以下验证码完成邮箱验证：\n\n\
         \x20\x20{token}\n\n\
         如非本人操作请忽略此邮件。验证码 24 小时内有效。"
    );
    state
        .email_config
        .build_service()
        .send_text(email, subject, &body)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = EmailVerifyConfirm)]
pub(crate) struct ConfirmParams {
    /// 验证 token（UUID 格式）
    #[validate(custom(function = "x_uuid"))]
    pub token: String,
}

/// 确认：用 token 取出 user_id，写入 `email_verified_at = NOW()`。
#[utoipa::path(
    post,
    path = "/users/email/verify/confirm",
    tag = "user",
    request_body = ConfirmParams,
    responses(
        (status = 204, description = "邮箱验证成功"),
        (status = 401, description = "token 无效或已过期")
    )
)]
pub(crate) async fn confirm_verify(
    State(state): State<EmailVerifyState>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<ConfirmParams>,
) -> Result<StatusCode> {
    let key = format!("{REDIS_PREFIX}{}", params.token);
    // 取出即删（GETDEL）：验证码是一次性的，读与删之间不留重放窗口
    let pending: Option<Vec<u8>> = state.cache.get_del(&key).await?;
    let pending: PendingVerification = pending
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .context(InvalidTokenSnafu)?;
    let user_id = pending.user_id;

    let marked = UserModel::new()
        .mark_email_verified(state.pool, user_id, &pending.email)
        .await?;
    if !marked {
        return Err(Error::EmailChanged.into());
    }

    // 审计：邮箱验证成功
    let _ = AuditLogModel::new()
        .log(
            state.pool,
            AuditLogParams::new("user.email_verify")
                .with_user(user_id)
                .with_target("user", user_id.to_string())
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(&headers)),
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}
