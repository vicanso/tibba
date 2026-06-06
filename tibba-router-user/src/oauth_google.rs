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

//! Google OAuth 路由：
//! - `GET /oauth/google/start`    —— 生成 state、存 Redis、302 → Google authorize
//! - `GET /oauth/google/callback` —— 校验 state、换 token、查/合并用户、建 Session、302 回 `/`
//!
//! 与 GitHub 流程完全对称，仅差异：
//! - provider = "google"
//! - provider_user_id = Google `sub` 字符串（非数字 id）
//! - account 名前缀 `g_`，无 username 字段，用 email 前缀生成
//! - state Redis key 前缀 `oauth_state:google:`，与 GitHub 隔离

use crate::{Result, user_agent_of};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Redirect;
use serde::Deserialize;
use serde_json::json;
use snafu::{OptionExt, Snafu};
use sqlx::PgPool;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_model::{Model, ROLE_SUPER_ADMIN, UserModel};
use tibba_model_builtin::{
    AuditLogModel, AuditLogParams, CreateLinkParams, RolePermissionModel, UserOauthLinkModel,
};
use tibba_oauth::{GoogleUser, OAuthConfig};
use tibba_session::{Session, SessionResponse};
use tibba_util::{sha256, uuid};
use tracing::warn;

const ERROR_CATEGORY: &str = "oauth_google";
const LOG_TARGET: &str = "tibba:oauth_google";
const PROVIDER: &str = "google";
const STATE_REDIS_PREFIX: &str = "oauth_state:google:";
const STATE_TTL_SECS: u64 = 10 * 60;
const ACCOUNT_PREFIX: &str = "g_";
const MAX_ACCOUNT_RETRY: u8 = 5;

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    #[snafu(display("invalid oauth callback params"))]
    InvalidParams,
    #[snafu(display("oauth state invalid or expired"))]
    StateInvalid,
    #[snafu(display("could not allocate unique account name"))]
    AccountAllocationFailed,
    #[snafu(display("linked local user not found"))]
    LinkedUserMissing,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::InvalidParams => BaseError::new("invalid oauth callback params")
                .with_sub_category("invalid_params")
                .with_status(400)
                .with_exception(false),
            Error::StateInvalid => BaseError::new("oauth state invalid or expired")
                .with_sub_category("state_invalid")
                .with_status(401)
                .with_exception(false),
            Error::AccountAllocationFailed => {
                BaseError::new("could not allocate unique account name")
                    .with_sub_category("account_allocation_failed")
                    .with_status(503)
                    .with_exception(true)
            }
            Error::LinkedUserMissing => BaseError::new("linked local user not found")
                .with_sub_category("linked_user_missing")
                .with_status(401)
                .with_exception(false),
        };
        err.with_category(ERROR_CATEGORY)
    }
}

#[derive(Clone)]
pub(crate) struct OauthGoogleState {
    pub pool: &'static PgPool,
    pub cache: &'static RedisCache,
    pub oauth_config: &'static OAuthConfig,
    /// 成功登录后重定向回的前端地址。空串时跳 `/`。
    pub success_redirect: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
}

/// `GET /oauth/google/start` —— 生成 state 存 Redis，302 → Google。
pub(crate) async fn start_login(State(state): State<OauthGoogleState>) -> Result<Redirect> {
    let provider = state.oauth_config.google.build_provider()?;
    let csrf_state = uuid();
    state
        .cache
        .set_struct(
            &format!("{STATE_REDIS_PREFIX}{csrf_state}"),
            &true,
            Some(Duration::from_secs(STATE_TTL_SECS)),
        )
        .await?;
    Ok(Redirect::temporary(&provider.authorize_url(&csrf_state)))
}

/// `GET /oauth/google/callback` —— 校验 state、换 token、user landing、建 Session、302。
pub(crate) async fn callback(
    State(state): State<OauthGoogleState>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Query(params): Query<CallbackParams>,
    session: Session,
) -> Result<SessionResponse<Redirect>> {
    let code = params.code.context(InvalidParamsSnafu)?;
    let csrf_state = params.state.context(InvalidParamsSnafu)?;

    let state_key = format!("{STATE_REDIS_PREFIX}{csrf_state}");
    let stored: Option<bool> = state.cache.get_struct(&state_key).await?;
    stored.context(StateInvalidSnafu)?;
    if let Err(e) = state.cache.del(&state_key).await {
        warn!(target: LOG_TARGET, error = %e, "delete used state failed");
    }

    let provider = state.oauth_config.google.build_provider()?;
    let google_user = provider.exchange_and_fetch(&code).await?;

    let user = land_user(state.pool, &google_user).await?;

    let groups = user.groups.clone().unwrap_or_default();
    let roles = user.roles.clone().unwrap_or_default();
    let permissions = RolePermissionModel::new()
        .list_permissions_for_roles(state.pool, &roles)
        .await
        .unwrap_or_default();

    let session = session
        .with_account(&user.account, user.id)
        .with_groups(groups)
        .with_roles(roles)
        .with_permissions(permissions);
    session.save().await?;

    if let Err(e) = UserModel::new()
        .update_last_login_at(state.pool, &user.account)
        .await
    {
        warn!(target: LOG_TARGET, error = %e, "update_last_login_at failed");
    }

    // 审计：OAuth 登录成功，detail 携带 provider 用于按渠道聚合
    let _ = AuditLogModel::new()
        .log(
            state.pool,
            AuditLogParams::new("user.oauth_login")
                .with_user(user.id)
                .with_target("user", user.id.to_string())
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(&headers))
                .with_detail(json!({ "provider": "google" })),
        )
        .await;

    let target = if state.success_redirect.is_empty() {
        "/".to_string()
    } else {
        state.success_redirect.clone()
    };
    Ok(SessionResponse(session, Redirect::temporary(&target)))
}

/// 三档落地：已绑定 → 邮箱合并 → 新建。
async fn land_user(
    pool: &PgPool,
    g: &GoogleUser,
) -> std::result::Result<tibba_model::User, BaseError> {
    let user_model = UserModel::new();
    let link_model = UserOauthLinkModel::new();
    let provider_user_id = g.sub.as_str();

    // 档一：已绑定
    if let Some(link) = link_model
        .find_by_provider_uid(pool, PROVIDER, provider_user_id)
        .await?
    {
        let user = user_model
            .get_by_id(pool, link.user_id as u64)
            .await?
            .context(LinkedUserMissingSnafu)?;
        return Ok(user);
    }

    // 档二：自动合并（verified email 命中本地）
    if let Some(email) = g.primary_verified_email.as_deref()
        && let Some(existing) = user_model.get_by_email(pool, email).await?
    {
        link_model
            .create(
                pool,
                CreateLinkParams {
                    user_id: existing.id,
                    provider: PROVIDER,
                    provider_user_id,
                    provider_login: "",
                    provider_email: email,
                },
            )
            .await?;
        return Ok(existing);
    }

    // 档三：新建
    let account = allocate_account(pool, g).await?;
    let random_password = sha256(format!("{}{}", uuid(), uuid()).as_bytes());
    let new_id = user_model.register(pool, &account, &random_password).await?;

    if new_id == 1 {
        user_model
            .update_by_id(pool, new_id, json!({ "roles": [ROLE_SUPER_ADMIN] }))
            .await?;
    }

    if let Some(email) = g.primary_verified_email.as_deref() {
        user_model
            .update_by_id(pool, new_id, json!({ "email": email }))
            .await?;
        user_model.mark_email_verified(pool, new_id as i64).await?;
    }

    link_model
        .create(
            pool,
            CreateLinkParams {
                user_id: new_id as i64,
                provider: PROVIDER,
                provider_user_id,
                provider_login: "",
                provider_email: g.primary_verified_email.as_deref().unwrap_or(""),
            },
        )
        .await?;

    let user = user_model
        .get_by_id(pool, new_id)
        .await?
        .context(LinkedUserMissingSnafu)?;
    Ok(user)
}

/// account 名分配：
/// - 优先 `g_{email_local}`（email "@" 前的部分，清洗后取 ≤17 字符）
/// - 无 email 时回退到 `g_{sub_前缀}`
/// - 冲突时改 `g_{短前缀}_{4 位随机数}`，最多 5 次
async fn allocate_account(
    pool: &PgPool,
    g: &GoogleUser,
) -> std::result::Result<String, BaseError> {
    let model = UserModel::new();

    let raw: String = g
        .primary_verified_email
        .as_deref()
        .and_then(|e| e.split('@').next())
        .map(|s| s.to_string())
        .unwrap_or_else(|| g.sub.clone());

    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(17)
        .collect();

    let base = format!("{ACCOUNT_PREFIX}{cleaned}");
    if model.get_by_account(pool, &base).await?.is_none() {
        return Ok(base);
    }

    let short_cleaned: String = cleaned.chars().take(9).collect();
    let short_base = format!("{ACCOUNT_PREFIX}{short_cleaned}");
    for _ in 0..MAX_ACCOUNT_RETRY {
        let raw_id = uuid();
        let suffix: u16 = raw_id
            .bytes()
            .take(2)
            .fold(0u16, |a, b| a.wrapping_mul(31).wrapping_add(b as u16))
            % 10000;
        let candidate = format!("{short_base}_{suffix:04}");
        if model.get_by_account(pool, &candidate).await?.is_none() {
            return Ok(candidate);
        }
    }
    Err(Error::AccountAllocationFailed.into())
}
