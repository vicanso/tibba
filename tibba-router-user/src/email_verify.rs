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

use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use snafu::{OptionExt, Snafu};
use sqlx::PgPool;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_email::EmailConfig;
use tibba_error::Error as BaseError;
use tibba_model::{Model, UserModel};
use tibba_session::UserSession;
use tibba_util::{JsonParams, uuid};
use tibba_validator::x_uuid;
use tracing::warn;
use validator::Validate;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "email_verify";
const REDIS_PREFIX: &str = "email_verify:";
const TOKEN_TTL_SECS: u64 = 24 * 60 * 60;
const LOG_TARGET: &str = "tibba:email_verify";

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// 用户没有绑定邮箱，无法发送验证邮件（HTTP 400）
    #[snafu(display("no email bound to this account"))]
    NoEmailOnAccount,
    /// Token 不存在或已过期（HTTP 401）
    #[snafu(display("invalid or expired token"))]
    InvalidToken,
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
            &user.id,
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

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct ConfirmParams {
    /// 验证 token（UUID 格式）
    #[validate(custom(function = "x_uuid"))]
    pub token: String,
}

/// 确认：用 token 取出 user_id，写入 `email_verified_at = NOW()`。
pub(crate) async fn confirm_verify(
    State(state): State<EmailVerifyState>,
    JsonParams(params): JsonParams<ConfirmParams>,
) -> Result<StatusCode> {
    let key = format!("{REDIS_PREFIX}{}", params.token);
    let user_id: Option<i64> = state.cache.get_struct(&key).await?;
    let user_id = user_id.context(InvalidTokenSnafu)?;

    UserModel::new()
        .mark_email_verified(state.pool, user_id)
        .await?;

    // 异步删 token——失败不阻断主流程（token 自身 24h 内会过期）
    if let Err(e) = state.cache.del(&key).await {
        warn!(target: LOG_TARGET, error = %e, "delete used token failed");
    }

    Ok(StatusCode::NO_CONTENT)
}
