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

//! GitHub OAuth 路由：
//! - `GET /oauth/github/start`    —— 生成 state、存 Redis、302 → GitHub authorize
//! - `GET /oauth/github/callback` —— 校验 state、换 token、查/合并用户、建 Session、302 回 `/`
//!
//! ## 用户落地策略（按设计 Q1=a / Q2=α）
//!
//! 1. **已绑定** —— `user_oauth_links` 命中 `(github, gh_id)` → 直接登录关联的本地用户
//! 2. **自动合并** —— GitHub `primary_verified_email` 命中本地 `users.email`
//!    → 新建 link 行，登录该用户（GitHub 端 verified=true 是合并安全前提）
//! 3. **新建用户** —— 否则 register 新 user：
//!    - account 名：`gh_{login}`；冲突时后缀随机数字直到 5 次失败
//!    - password：64 字符 sha256(uuid+uuid)，用户不可知；后续可走密码重置设值
//!    - 如有 verified email → 写入 `users.email` + `email_verified_at = NOW()`
//! 4. 写 `user_oauth_links` 行（provider="github" + GitHub 数字 id）
//! 5. 拉 roles → permissions 注入 Session、save、302 回前端

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
use tibba_oauth::{GitHubUser, OAuthConfig};
use tibba_session::{Session, SessionResponse};
use tibba_util::{sha256, uuid};
use tracing::warn;

const ERROR_CATEGORY: &str = "oauth_github";
const LOG_TARGET: &str = "tibba:oauth_github";
const PROVIDER: &str = "github";
const STATE_REDIS_PREFIX: &str = "oauth_state:github:";
const STATE_TTL_SECS: u64 = 10 * 60;
const ACCOUNT_PREFIX: &str = "gh_";
const MAX_ACCOUNT_RETRY: u8 = 5;

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// 缺 code / state —— GitHub 回调参数异常或被篡改（HTTP 400）
    #[snafu(display("invalid oauth callback params"))]
    InvalidParams,
    /// state 不在 Redis（过期、被重用或伪造）—— HTTP 401
    #[snafu(display("oauth state invalid or expired"))]
    StateInvalid,
    /// 5 次重试都凑不出唯一 account 名 —— 极小概率（HTTP 503）
    #[snafu(display("could not allocate unique account name"))]
    AccountAllocationFailed,
    /// 已绑定 GitHub 用户的本地 user 已被软删除（HTTP 401）
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

/// 共享 state：DB / Redis / OAuth 配置 / 成功回跳前端地址。
#[derive(Clone)]
pub(crate) struct OauthGitHubState {
    pub pool: &'static PgPool,
    pub cache: &'static RedisCache,
    pub oauth_config: &'static OAuthConfig,
    /// 成功登录后重定向回的前端地址。空串时回 "/"。
    pub success_redirect: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
}

/// `GET /oauth/github/start` —— 生成 state 存 Redis，302 → GitHub。
/// provider 未配置时返回 503（带明确 sub_category），不静默失败。
pub(crate) async fn start_login(State(state): State<OauthGitHubState>) -> Result<Redirect> {
    let provider = state.oauth_config.github.build_provider()?;
    let csrf_state = uuid();
    state
        .cache
        .set_struct(
            &format!("{STATE_REDIS_PREFIX}{csrf_state}"),
            // value 用占位符；存在即可表示 state 已发放
            &true,
            Some(Duration::from_secs(STATE_TTL_SECS)),
        )
        .await?;
    Ok(Redirect::temporary(&provider.authorize_url(&csrf_state)))
}

/// `GET /oauth/github/callback` —— 校验 state、换 token、user landing、建 Session、302。
pub(crate) async fn callback(
    State(state): State<OauthGitHubState>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Query(params): Query<CallbackParams>,
    session: Session,
) -> Result<SessionResponse<Redirect>> {
    let code = params.code.context(InvalidParamsSnafu)?;
    let csrf_state = params.state.context(InvalidParamsSnafu)?;

    // 校验并立即作废 state，防重放
    let state_key = format!("{STATE_REDIS_PREFIX}{csrf_state}");
    let stored: Option<bool> = state.cache.get_struct(&state_key).await?;
    stored.context(StateInvalidSnafu)?;
    if let Err(e) = state.cache.del(&state_key).await {
        warn!(target: LOG_TARGET, error = %e, "delete used state failed");
    }

    let provider = state.oauth_config.github.build_provider()?;
    let github_user = provider.exchange_and_fetch(&code).await?;

    let user = land_user(state.pool, &github_user).await?;

    // 角色 → 权限，复用 login 流程的注入逻辑
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

    // 同步更新 last_login_at，失败仅日志
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
                .with_detail(json!({ "provider": "github" })),
        )
        .await;

    let target = if state.success_redirect.is_empty() {
        "/".to_string()
    } else {
        state.success_redirect.clone()
    };
    Ok(SessionResponse(session, Redirect::temporary(&target)))
}

/// 用户落地：按已绑定 → 邮箱合并 → 新建 三档分发。
async fn land_user(
    pool: &PgPool,
    gh: &GitHubUser,
) -> std::result::Result<tibba_model::User, BaseError> {
    let user_model = UserModel::new();
    let link_model = UserOauthLinkModel::new();
    let provider_user_id = gh.id.to_string();

    // 档一：已绑定
    if let Some(link) = link_model
        .find_by_provider_uid(pool, PROVIDER, &provider_user_id)
        .await?
    {
        let user = user_model
            .get_by_id(pool, link.user_id as u64)
            .await?
            .context(LinkedUserMissingSnafu)?;
        return Ok(user);
    }

    // 档二：自动合并（verified email 命中本地）
    if let Some(email) = gh.primary_verified_email.as_deref()
        && let Some(existing) = user_model.get_by_email(pool, email).await?
    {
        link_model
            .create(
                pool,
                CreateLinkParams {
                    user_id: existing.id,
                    provider: PROVIDER,
                    provider_user_id: &provider_user_id,
                    provider_login: &gh.login,
                    provider_email: email,
                },
            )
            .await?;
        return Ok(existing);
    }

    // 档三：新建
    let account = allocate_account(pool, &gh.login).await?;
    // 64 字符 sha256，用户不可知，满足 x_user_password 验证（≥32 chars）
    let random_password = sha256(format!("{}{}", uuid(), uuid()).as_bytes());
    let new_id = user_model.register(pool, &account, &random_password).await?;

    // 首个注册用户自动 ROLE_SUPER_ADMIN（与表单注册流程对齐）
    if new_id == 1 {
        user_model
            .update_by_id(pool, new_id, json!({ "roles": [ROLE_SUPER_ADMIN] }))
            .await?;
    }

    // 有 verified email 直接写入 + 标记已验证
    if let Some(email) = gh.primary_verified_email.as_deref() {
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
                provider_user_id: &provider_user_id,
                provider_login: &gh.login,
                provider_email: gh.primary_verified_email.as_deref().unwrap_or(""),
            },
        )
        .await?;

    let user = user_model
        .get_by_id(pool, new_id)
        .await?
        .context(LinkedUserMissingSnafu)?;
    Ok(user)
}

/// account 名分配：`gh_{login}` → 冲突时 `gh_{短 login}_{4位数字}` 最多 5 次。
/// x_user_account 上限 20 字符 → 前缀 "gh_"(3) + 短 login(≤9) + "_NNNN"(5) = 17，安全。
async fn allocate_account(pool: &PgPool, login: &str) -> std::result::Result<String, BaseError> {
    let model = UserModel::new();
    // 清洗 login：只保留 ASCII；首次尝试用最多 17 字符（前缀 3 + 17 = 20，正好上限）
    let cleaned: String = login
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(17)
        .collect();
    let base = format!("{ACCOUNT_PREFIX}{cleaned}");
    if model.get_by_account(pool, &base).await?.is_none() {
        return Ok(base);
    }

    // 冲突走带后缀：base 必须更短以容纳 "_NNNN"
    let short_cleaned: String = cleaned.chars().take(9).collect();
    let short_base = format!("{ACCOUNT_PREFIX}{short_cleaned}");
    for _ in 0..MAX_ACCOUNT_RETRY {
        let raw = uuid();
        let suffix: u16 = raw
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
