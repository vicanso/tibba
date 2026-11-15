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

//! GitHub OAuth 2.0 Authorization Code 流程实现。
//!
//! 三个 HTTP 调用：
//! 1. **Authorize redirect**（无需 HTTP，仅生成 URL）
//!    `https://github.com/login/oauth/authorize?client_id=...&redirect_uri=...&state=...&scope=user:email`
//! 2. **Token exchange**
//!    `POST https://github.com/login/oauth/access_token`（Accept: application/json）
//!    返回 `{ access_token, token_type, scope }`
//! 3. **User info**
//!    `GET https://api.github.com/user` + `GET https://api.github.com/user/emails`
//!
//! GitHub 要求每个请求带 `User-Agent`，否则 403。

use crate::{ApiSnafu, Error, HttpSnafu, JsonSnafu, LOG_TARGET, format_error_body};
use http::Method;
use http::header::{ACCEPT, HeaderValue, USER_AGENT};
use serde::Deserialize;
use snafu::{OptionExt, ResultExt};
use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;
use tibba_request::{Client, ClientBuilder, Params};
use tracing::debug;

const AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_URL: &str = "https://api.github.com/user";
const EMAILS_URL: &str = "https://api.github.com/user/emails";
const USER_AGENT_VALUE: &str = "tibba-oauth";
const DEFAULT_SCOPE: &str = "read:user user:email";
const HTTP_TIMEOUT_SECS: u64 = 15;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

/// 已配置的 GitHub OAuth 客户端。一次配置反复使用——客户端是无状态的。
#[derive(Clone)]
pub struct GitHubProvider {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

impl fmt::Debug for GitHubProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubProvider")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &if self.client_secret.is_empty() {
                    ""
                } else {
                    "[REDACTED]"
                },
            )
            .field("redirect_uri", &self.redirect_uri)
            .finish()
    }
}

/// 第三方（GitHub）侧的用户身份信息汇总。`primary_verified_email`
/// 只在 GitHub 端 verified=true 时才填充，从而满足自动合并的安全前提。
#[derive(Debug, Clone)]
pub struct GitHubUser {
    /// GitHub 数字 id，用作 `user_oauth_links.provider_user_id`
    pub id: i64,
    /// GitHub username（可改名）
    pub login: String,
    /// 显示名（可空）
    pub name: Option<String>,
    /// 头像 URL
    pub avatar_url: Option<String>,
    /// 主邮箱，**仅当 GitHub 标记 verified=true 时**才有值
    pub primary_verified_email: Option<String>,
}

impl GitHubProvider {
    #[must_use]
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
        }
    }

    /// 生成 OAuth authorize URL。`state` 由调用方生成并存储（Redis），
    /// 在 callback 中比对，防 CSRF / 防回放。
    #[must_use]
    pub fn authorize_url(&self, state: &str) -> String {
        // 按 GitHub 文档，所有参数都要 URL 编码
        format!(
            "{AUTHORIZE_URL}?client_id={cid}&redirect_uri={ruri}&state={st}&scope={scope}&response_type=code",
            cid = urlencoding::encode(&self.client_id),
            ruri = urlencoding::encode(&self.redirect_uri),
            st = urlencoding::encode(state),
            scope = urlencoding::encode(DEFAULT_SCOPE),
        )
    }

    /// 用 code 换 access_token，然后拉取用户信息 + verified 邮箱。
    /// 调用方应已校验过 state，这里只做 OAuth 协议本身。
    pub async fn exchange_and_fetch(&self, code: &str) -> Result<GitHubUser, Error> {
        let http = http_client()?;
        let access_token = self.exchange_code(&http, code).await?;
        let user = self.fetch_user(&http, &access_token).await?;
        if user.id <= 0 {
            return Err(Error::Api {
                provider: "github".to_string(),
                status: 200,
                message: "provider returned non-positive user id".to_string(),
            });
        }
        let primary_verified_email = match self
            .fetch_primary_verified_email(&http, &access_token)
            .await
        {
            Ok(email) => email,
            // 仅 403（未授 email scope）或 404 允许无邮箱继续；网络/5xx 错误正常上报
            Err(Error::Api { status, .. }) if status == 403 || status == 404 => None,
            Err(err) => return Err(err),
        };
        debug!(
            target: LOG_TARGET,
            login = %user.login,
            has_email = primary_verified_email.is_some(),
            "github oauth user fetched"
        );
        Ok(GitHubUser {
            id: user.id,
            login: user.login,
            name: user.name,
            avatar_url: user.avatar_url,
            primary_verified_email,
        })
    }

    async fn exchange_code(&self, http: &Client, code: &str) -> Result<String, Error> {
        #[derive(Deserialize)]
        struct TokenResp {
            access_token: Option<String>,
            #[serde(default)]
            error: Option<String>,
            #[serde(default)]
            error_description: Option<String>,
        }

        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", self.redirect_uri.as_str()),
        ];
        let resp = http
            .execute(
                Params::new(Method::POST, TOKEN_URL)
                    .with_header(ACCEPT, HeaderValue::from_static("application/json"))
                    .with_form(&params),
            )
            .await
            .context(HttpSnafu)?;
        if !resp.is_success() {
            let msg = format_error_body(&resp.body);
            if matches!(resp.status, 400 | 401 | 403 | 422) {
                return Err(Error::Rejected {
                    provider: "github".to_string(),
                    message: msg,
                });
            }
            return Err(Error::Api {
                provider: "github".to_string(),
                status: resp.status,
                message: msg,
            });
        }
        let parsed: TokenResp = serde_json::from_slice(&resp.body).context(JsonSnafu)?;
        if let Some(err) = parsed.error {
            return Err(Error::Rejected {
                provider: "github".to_string(),
                message: parsed.error_description.unwrap_or(err),
            });
        }
        parsed.access_token.context(ApiSnafu {
            provider: "github".to_string(),
            status: resp.status,
            message: "token response missing access_token",
        })
    }

    async fn fetch_user(&self, http: &Client, access_token: &str) -> Result<RawUser, Error> {
        #[derive(Deserialize)]
        struct WithMessage {
            #[serde(default)]
            message: Option<String>,
        }
        let resp = http
            .execute(
                Params::new(Method::GET, USER_URL)
                    .with_bearer_auth(access_token)
                    .with_header(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE))
                    .with_header(
                        ACCEPT,
                        HeaderValue::from_static("application/vnd.github+json"),
                    ),
            )
            .await
            .context(HttpSnafu)?;
        if !resp.is_success() {
            let msg = serde_json::from_slice::<WithMessage>(&resp.body)
                .ok()
                .and_then(|w| w.message)
                .unwrap_or_else(|| format_error_body(&resp.body));
            return Err(Error::Api {
                provider: "github".to_string(),
                status: resp.status,
                message: msg,
            });
        }
        serde_json::from_slice::<RawUser>(&resp.body).context(JsonSnafu)
    }

    async fn fetch_primary_verified_email(
        &self,
        http: &Client,
        access_token: &str,
    ) -> Result<Option<String>, Error> {
        let resp = http
            .execute(
                Params::new(Method::GET, EMAILS_URL)
                    .with_bearer_auth(access_token)
                    .with_header(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE))
                    .with_header(
                        ACCEPT,
                        HeaderValue::from_static("application/vnd.github+json"),
                    ),
            )
            .await
            .context(HttpSnafu)?;
        if !resp.is_success() {
            return Err(Error::Api {
                provider: "github".to_string(),
                status: resp.status,
                message: format_error_body(&resp.body),
            });
        }
        let emails: Vec<RawEmail> = serde_json::from_slice(&resp.body).context(JsonSnafu)?;
        // 自动合并的关键安全前提：必须 primary=true 且 verified=true
        Ok(emails
            .into_iter()
            .find(|e| e.primary && e.verified)
            .map(|e| e.email))
    }
}

/// 懒初始化复用 [`tibba_request::Client`] 连接池；显式设置 timeout 防 hang，
/// 避免外部依赖把 OAuth callback 卡死。
fn http_client() -> Result<Client, Error> {
    if let Some(client) = HTTP_CLIENT.get() {
        return Ok(client.clone());
    }
    let client = ClientBuilder::new("oauth-github")
        .with_timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .with_common_interceptor()
        .build()
        .context(HttpSnafu)?;
    let _ = HTTP_CLIENT.set(client.clone());
    Ok(HTTP_CLIENT.get().cloned().unwrap_or(client))
}

#[derive(Deserialize)]
struct RawUser {
    id: i64,
    login: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct RawEmail {
    email: String,
    primary: bool,
    verified: bool,
}
