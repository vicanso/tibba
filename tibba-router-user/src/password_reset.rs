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

//! 密码重置流程：
//! - `POST /password/reset/request` — 匿名按账号请求；**总是返回 204**，
//!   不管账号是否存在 / 是否绑定邮箱，防止账号枚举攻击
//! - `POST /password/reset/confirm` — 提交 token + 新密码完成重置
//!
//! Token 用 UUID（36 字符），存 Redis 1h，单次使用后立即 del。

use crate::user_agent_of;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use snafu::{OptionExt, Snafu};
use sqlx::PgPool;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_email::EmailConfig;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_model::{Model, UserModel};
use tibba_model_builtin::{AuditLogModel, AuditLogParams};
use tibba_util::JsonParams;
use tibba_util::uuid;
use tibba_validator::{x_user_account, x_user_password, x_uuid};
use tracing::warn;
use utoipa::ToSchema;
use validator::Validate;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "password_reset";
const REDIS_PREFIX: &str = "password_reset:";
const TOKEN_TTL_SECS: u64 = 60 * 60;
const LOG_TARGET: &str = "tibba:password_reset";

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// Token 不存在或已过期（HTTP 401）
    #[snafu(display("invalid or expired token"))]
    InvalidToken,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::InvalidToken => BaseError::new("invalid or expired token")
                .with_sub_category("invalid_token")
                .with_status(401)
                .with_exception(false),
        };
        err.with_category(ERROR_CATEGORY)
    }
}

#[derive(Clone)]
pub(crate) struct PasswordResetState {
    pub pool: &'static PgPool,
    pub cache: &'static RedisCache,
    pub email_config: &'static EmailConfig,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub(crate) struct RequestParams {
    /// 账号
    #[validate(custom(function = "x_user_account"))]
    pub account: String,
}

/// 触发：匿名调用按账号请求重置邮件。
///
/// **安全性**：无论账号是否存在、是否绑定邮箱、邮件是否发送成功，
/// 端点都返回 204。失败原因只写入服务端日志，**绝不**透传给客户端，
/// 避免攻击者用响应差异枚举有效账号。
#[utoipa::path(
    post,
    path = "/users/password/reset/request",
    tag = "user",
    request_body = RequestParams,
    responses((status = 204, description = "总是返回 204（防账号枚举），不透传账号是否存在"))
)]
pub(crate) async fn request_reset(
    State(state): State<PasswordResetState>,
    JsonParams(params): JsonParams<RequestParams>,
) -> Result<StatusCode> {
    // 一切错误都吞掉走日志——返回 204 是约定，不能被失败情况影响
    if let Err(e) = try_send_reset_email(&state, &params.account).await {
        warn!(
            target: LOG_TARGET,
            account = %params.account,
            error = %e,
            "send reset email failed (suppressed)"
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 内部封装，所有失败都被 [`request_reset`] 吞掉。分离出来仅为方便 `?` 用法。
async fn try_send_reset_email(state: &PasswordResetState, account: &str) -> Result<()> {
    let Some(user) = UserModel::new().get_by_account(state.pool, account).await? else {
        return Ok(());
    };
    let Some(email) = user.email.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(());
    };

    let token = uuid();
    state
        .cache
        .set_struct(
            &format!("{REDIS_PREFIX}{token}"),
            &user.id,
            Some(Duration::from_secs(TOKEN_TTL_SECS)),
        )
        .await?;

    let subject = "重置您的密码";
    let body = format!(
        "您好，\n\n\
         我们收到了重置账号 {account} 密码的请求。请使用以下验证码：\n\n\
         \x20\x20{token}\n\n\
         如非本人操作请忽略此邮件。验证码 1 小时内有效。"
    );
    state
        .email_config
        .build_service()
        .send_text(email, subject, &body)
        .await?;
    Ok(())
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = PasswordResetConfirm)]
pub(crate) struct ConfirmParams {
    /// 重置 token（UUID 格式）
    #[validate(custom(function = "x_uuid"))]
    pub token: String,
    /// 新密码：客户端已 sha256 处理（与 register 一致）
    #[validate(custom(function = "x_user_password"))]
    pub password: String,
}

/// 确认：用 token 拿到 user_id，覆盖密码。
#[utoipa::path(
    post,
    path = "/users/password/reset/confirm",
    tag = "user",
    request_body = ConfirmParams,
    responses(
        (status = 204, description = "密码重置成功"),
        (status = 401, description = "token 无效或已过期")
    )
)]
pub(crate) async fn confirm_reset(
    State(state): State<PasswordResetState>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<ConfirmParams>,
) -> Result<StatusCode> {
    let key = format!("{REDIS_PREFIX}{}", params.token);
    let user_id: Option<i64> = state.cache.get_struct(&key).await?;
    let user_id = user_id.context(InvalidTokenSnafu)?;

    UserModel::new()
        .update_password(state.pool, user_id, &params.password)
        .await?;

    // 异步删 token——失败不阻断主流程（token 自身 1h 内会过期）
    if let Err(e) = state.cache.del(&key).await {
        warn!(target: LOG_TARGET, error = %e, "delete used token failed");
    }

    // 审计：密码重置成功。**detail 不带新旧密码**，仅记动作完成
    let _ = AuditLogModel::new()
        .log(
            state.pool,
            AuditLogParams::new("user.password_reset")
                .with_user(user_id)
                .with_target("user", user_id.to_string())
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(&headers)),
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}
