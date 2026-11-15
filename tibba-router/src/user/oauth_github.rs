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
//! - `GET /oauth/github/start`    —— 生成 state 与浏览器绑定 nonce Cookie、存 Redis、302 → GitHub authorize
//! - `GET /oauth/github/callback` —— 校验 state+nonce、换 token、单事务落地用户、2FA 闸门或建 Session 并安全回跳
//!
//! ## 用户落地策略
//!
//! 1. **已绑定** —— `user_oauth_links` 命中 `(github, gh_id)` → 直接登录关联的本地用户
//! 2. **自动合并** —— GitHub `primary_verified_email` 命中本地 `users.email`
//!    → 新建 link 行，登录该用户（GitHub 端 verified=true 是合并安全前提）
//! 3. **新建用户** —— 单事务注册用户并写入 link 行：
//!    - account 名：清洗 `gh_{login}`（空值回退 `gh_user`）；冲突时 `nanoid` 随机后缀最多 5 次
//!    - 如有 verified email → 同事务写入 `users.email` + `email_verified_at = NOW()`
//! 4. **2FA 闸门与防开放重定向** —— 若已启用 2FA 则签发短期挑战并跳转到站内安全路径，否则写 Session 并回跳

use super::{
    OAuthCompleteParams, OAuthLandingSpec, Result, build_oauth_nonce_cookie, complete_oauth_login,
    land_oauth_user, verify_oauth_state_and_nonce,
};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{Redirect, Response};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use snafu::{OptionExt, Snafu};
use sqlx::PgPool;
use std::time::Duration;
use tibba_auth::Session;
use tibba_auth::oauth::OAuthConfig;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_util::uuid;
use utoipa::IntoParams;

const ERROR_CATEGORY: &str = "oauth_github";
const PROVIDER: &str = "github";
const STATE_REDIS_PREFIX: &str = "oauth_state:github:";
const STATE_TTL_SECS: u64 = 10 * 60;
const ACCOUNT_PREFIX: &str = "gh_";

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// 缺 code / state —— GitHub 回调参数异常或被篡改（HTTP 400）
    #[snafu(display("invalid oauth callback params"))]
    InvalidParams,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::InvalidParams => BaseError::new("invalid oauth callback params")
                .with_sub_category("invalid_params")
                .with_status(400)
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
    /// 成功登录后重定向回的前端站内相对地址。非法/空串时回 "/"。
    pub success_redirect: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct CallbackParams {
    /// GitHub 回调授权码
    code: Option<String>,
    /// 防 CSRF 的 state（须与 start 阶段发放的一致）
    state: Option<String>,
}

/// `GET /oauth/github/start` —— 生成 state 并绑定浏览器 nonce Cookie 存 Redis，302 → GitHub。
/// provider 未配置时返回 503（带明确 sub_category），不静默失败。
#[utoipa::path(
    get,
    path = "/users/oauth/github/start",
    tag = "user",
    operation_id = "oauth_github_start",
    responses(
        (status = 307, description = "302/307 重定向到 GitHub 授权页"),
        (status = 503, description = "GitHub OAuth 未配置")
    )
)]
pub(crate) async fn start_login(
    State(state): State<OauthGitHubState>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect)> {
    let provider = state.oauth_config.github.build_provider()?;
    let csrf_state = uuid();
    let browser_nonce = uuid();
    state
        .cache
        .set_struct(
            &format!("{STATE_REDIS_PREFIX}{csrf_state}"),
            &browser_nonce,
            Some(Duration::from_secs(STATE_TTL_SECS)),
        )
        .await?;
    let jar = jar.add(build_oauth_nonce_cookie(
        &browser_nonce,
        STATE_TTL_SECS as i64,
    ));
    Ok((
        jar,
        Redirect::temporary(&provider.authorize_url(&csrf_state)),
    ))
}

/// `GET /oauth/github/callback` —— 校验 state+nonce、换 token、user landing、2FA 闸门或建 Session、安全回跳。
#[utoipa::path(
    get,
    path = "/users/oauth/github/callback",
    tag = "user",
    operation_id = "oauth_github_callback",
    params(CallbackParams),
    responses(
        (status = 307, description = "登录成功或进入 MFA 第二步，安全重定向回站内前端"),
        (status = 401, description = "state 无效、浏览器绑定缺失或参数缺失")
    )
)]
pub(crate) async fn callback(
    State(state): State<OauthGitHubState>,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    jar: CookieJar,
    Query(params): Query<CallbackParams>,
    session: Session,
) -> Result<Response> {
    let code = params.code.context(InvalidParamsSnafu)?;
    let csrf_state = params.state.context(InvalidParamsSnafu)?;

    verify_oauth_state_and_nonce(
        state.cache,
        STATE_REDIS_PREFIX,
        &csrf_state,
        &jar,
        ERROR_CATEGORY,
    )
    .await?;

    let provider = state.oauth_config.github.build_provider()?;
    let github_user = provider.exchange_and_fetch(&code).await?;
    let provider_user_id = github_user.id.to_string();

    let user = land_oauth_user(
        state.pool,
        OAuthLandingSpec {
            provider: PROVIDER,
            account_prefix: ACCOUNT_PREFIX,
            account_hint: &github_user.login,
            provider_user_id: &provider_user_id,
            provider_login: &github_user.login,
            verified_email: github_user.primary_verified_email.as_deref(),
        },
        ERROR_CATEGORY,
    )
    .await?;

    complete_oauth_login(OAuthCompleteParams {
        pool: state.pool,
        cache: state.cache,
        session,
        user,
        provider: PROVIDER,
        success_redirect: &state.success_redirect,
        request_id: &request_id,
        ip: ip.to_string(),
        headers: &headers,
    })
    .await
}
