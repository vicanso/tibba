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
//! - `GET /oauth/google/start`    —— 生成 state 与浏览器绑定 nonce Cookie、存 Redis、302 → Google authorize
//! - `GET /oauth/google/callback` —— 校验 state+nonce、换 token、单事务落地用户、2FA 闸门或建 Session 并安全回跳
//!
//! 与 GitHub 流程共用 `lib.rs` 的 OAuth 状态校验、账户分配、事务落地与 2FA 闸门逻辑，仅差异：
//! - `provider = "google"`、`provider_user_id = Google sub`、`account_prefix = "g_"`
//! - state Redis key 前缀 `oauth_state:google:`，与 GitHub 隔离

use crate::{
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
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_oauth::OAuthConfig;
use tibba_session::Session;
use tibba_util::uuid;
use utoipa::IntoParams;

const ERROR_CATEGORY: &str = "oauth_google";
const PROVIDER: &str = "google";
const STATE_REDIS_PREFIX: &str = "oauth_state:google:";
const STATE_TTL_SECS: u64 = 10 * 60;
const ACCOUNT_PREFIX: &str = "g_";

#[derive(Debug, Snafu)]
pub(crate) enum Error {
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

#[derive(Clone)]
pub(crate) struct OauthGoogleState {
    pub pool: &'static PgPool,
    pub cache: &'static RedisCache,
    pub oauth_config: &'static OAuthConfig,
    /// 成功登录后重定向回的前端站内相对地址。非法/空串时跳 `/`。
    pub success_redirect: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct CallbackParams {
    /// Google 回调授权码
    code: Option<String>,
    /// 防 CSRF 的 state（须与 start 阶段发放的一致）
    state: Option<String>,
}

/// `GET /oauth/google/start` —— 生成 state 并绑定浏览器 nonce Cookie 存 Redis，302 → Google。
#[utoipa::path(
    get,
    path = "/users/oauth/google/start",
    tag = "user",
    operation_id = "oauth_google_start",
    responses(
        (status = 307, description = "302/307 重定向到 Google 授权页"),
        (status = 503, description = "Google OAuth 未配置")
    )
)]
pub(crate) async fn start_login(
    State(state): State<OauthGoogleState>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect)> {
    let provider = state.oauth_config.google.build_provider()?;
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

/// `GET /oauth/google/callback` —— 校验 state+nonce、换 token、user landing、2FA 闸门或建 Session、安全回跳。
#[utoipa::path(
    get,
    path = "/users/oauth/google/callback",
    tag = "user",
    operation_id = "oauth_google_callback",
    params(CallbackParams),
    responses(
        (status = 307, description = "登录成功或进入 MFA 第二步，安全重定向回站内前端"),
        (status = 401, description = "state 无效、浏览器绑定缺失或参数缺失")
    )
)]
pub(crate) async fn callback(
    State(state): State<OauthGoogleState>,
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

    let provider = state.oauth_config.google.build_provider()?;
    let google_user = provider.exchange_and_fetch(&code).await?;

    let account_hint = google_user
        .primary_verified_email
        .as_deref()
        .and_then(|e| e.split('@').next())
        .unwrap_or(&google_user.sub);

    let user = land_oauth_user(
        state.pool,
        OAuthLandingSpec {
            provider: PROVIDER,
            account_prefix: ACCOUNT_PREFIX,
            account_hint,
            provider_user_id: google_user.sub.as_str(),
            provider_login: "",
            verified_email: google_user.primary_verified_email.as_deref(),
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
