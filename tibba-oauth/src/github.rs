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

use crate::{ApiSnafu, Error, HttpSnafu, JsonSnafu, LOG_TARGET};
use serde::Deserialize;
use snafu::{OptionExt, ResultExt};
use std::time::Duration;
use tracing::debug;

const AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_URL: &str = "https://api.github.com/user";
const EMAILS_URL: &str = "https://api.github.com/user/emails";
const USER_AGENT: &str = "tibba-oauth";
const DEFAULT_SCOPE: &str = "read:user user:email";
const HTTP_TIMEOUT_SECS: u64 = 15;

/// 已配置的 GitHub OAuth 客户端。一次配置反复使用——客户端是无状态的。
#[derive(Debug, Clone)]
pub struct GitHubProvider {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
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
        let primary_verified_email = self
            .fetch_primary_verified_email(&http, &access_token)
            .await
            .ok()
            .flatten();
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

    async fn exchange_code(&self, http: &reqwest::Client, code: &str) -> Result<String, Error> {
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
            .post(TOKEN_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&params)
            .send()
            .await
            .context(HttpSnafu)?;
        let status = resp.status();
        let bytes = resp.bytes().await.context(HttpSnafu)?;
        if !status.is_success() {
            return Err(Error::Api {
                provider: "github".to_string(),
                status: status.as_u16(),
                message: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        let parsed: TokenResp = serde_json::from_slice(&bytes).context(JsonSnafu)?;
        if let Some(err) = parsed.error {
            return Err(Error::Api {
                provider: "github".to_string(),
                status: status.as_u16(),
                message: parsed.error_description.unwrap_or(err),
            });
        }
        parsed.access_token.context(ApiSnafu {
            provider: "github".to_string(),
            status: status.as_u16(),
            message: "token response missing access_token",
        })
    }

    async fn fetch_user(
        &self,
        http: &reqwest::Client,
        access_token: &str,
    ) -> Result<RawUser, Error> {
        #[derive(Deserialize)]
        struct WithMessage {
            #[serde(default)]
            message: Option<String>,
        }
        let resp = http
            .get(USER_URL)
            .bearer_auth(access_token)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .context(HttpSnafu)?;
        let status = resp.status();
        let bytes = resp.bytes().await.context(HttpSnafu)?;
        if !status.is_success() {
            let msg = serde_json::from_slice::<WithMessage>(&bytes)
                .ok()
                .and_then(|w| w.message)
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
            return Err(Error::Api {
                provider: "github".to_string(),
                status: status.as_u16(),
                message: msg,
            });
        }
        serde_json::from_slice::<RawUser>(&bytes).context(JsonSnafu)
    }

    async fn fetch_primary_verified_email(
        &self,
        http: &reqwest::Client,
        access_token: &str,
    ) -> Result<Option<String>, Error> {
        let resp = http
            .get(EMAILS_URL)
            .bearer_auth(access_token)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .context(HttpSnafu)?;
        let status = resp.status();
        let bytes = resp.bytes().await.context(HttpSnafu)?;
        if !status.is_success() {
            return Err(Error::Api {
                provider: "github".to_string(),
                status: status.as_u16(),
                message: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        let emails: Vec<RawEmail> = serde_json::from_slice(&bytes).context(JsonSnafu)?;
        // 自动合并的关键安全前提：必须 primary=true 且 verified=true
        Ok(emails
            .into_iter()
            .find(|e| e.primary && e.verified)
            .map(|e| e.email))
    }
}

/// HTTP 客户端构造：每次调用新建一个；reqwest 内部有连接池复用。
/// 显式设置 timeout 防 hang，避免外部依赖把 OAuth callback 卡死。
fn http_client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .context(HttpSnafu)
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
