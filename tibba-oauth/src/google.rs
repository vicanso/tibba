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

//! Google OAuth 2.0 / OpenID Connect 流程实现。
//!
//! 与 GitHub 的差异：
//! - **endpoint 不同**：authorize 走 `accounts.google.com`、token / userinfo 走 `googleapis.com`
//! - **响应是 JSON OIDC 标准字段**：`sub`（稳定 id，string） / `email` / `email_verified` / `name` / `picture`
//! - **不需要单独的 /emails 调用**：userinfo 一次性返回 email + verified 状态
//!
//! Google 要求 scope 含 `openid email profile` 才能在 userinfo 拿到上述字段。

use crate::{ApiSnafu, Error, HttpSnafu, JsonSnafu, LOG_TARGET};
use serde::Deserialize;
use snafu::{OptionExt, ResultExt};
use std::time::Duration;
use tracing::debug;

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v3/userinfo";
const DEFAULT_SCOPE: &str = "openid email profile";
const HTTP_TIMEOUT_SECS: u64 = 15;

/// 已配置的 Google OAuth 客户端。无状态，可复用。
#[derive(Debug, Clone)]
pub struct GoogleProvider {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

/// 第三方（Google）侧的用户身份信息汇总。`primary_verified_email`
/// 只在 Google 端 `email_verified=true` 时才填充，满足自动合并的安全前提。
#[derive(Debug, Clone)]
pub struct GoogleUser {
    /// Google OpenID `sub` 字符串，用作 `user_oauth_links.provider_user_id`
    pub sub: String,
    /// 主邮箱（未验证时不填，避免触发自动合并）
    pub primary_verified_email: Option<String>,
    /// 显示名（可空）
    pub name: Option<String>,
    /// 头像 URL
    pub picture: Option<String>,
}

impl GoogleProvider {
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

    /// 生成 OAuth authorize URL。`state` 由调用方生成并存储（Redis）。
    /// 额外加 `prompt=select_account` 让用户每次都看到账号选择器，避免被默认账号沉默登录。
    pub fn authorize_url(&self, state: &str) -> String {
        format!(
            "{AUTHORIZE_URL}?client_id={cid}&redirect_uri={ruri}&state={st}&scope={scope}&response_type=code&access_type=online&prompt=select_account",
            cid = urlencoding::encode(&self.client_id),
            ruri = urlencoding::encode(&self.redirect_uri),
            st = urlencoding::encode(state),
            scope = urlencoding::encode(DEFAULT_SCOPE),
        )
    }

    /// 用 code 换 access_token，然后拉取 userinfo。
    pub async fn exchange_and_fetch(&self, code: &str) -> Result<GoogleUser, Error> {
        let http = http_client()?;
        let access_token = self.exchange_code(&http, code).await?;
        let raw = self.fetch_userinfo(&http, &access_token).await?;

        let primary_verified_email = if raw.email_verified {
            raw.email.clone()
        } else {
            None
        };
        debug!(
            target: LOG_TARGET,
            sub = %raw.sub,
            has_email = primary_verified_email.is_some(),
            "google oauth user fetched"
        );
        Ok(GoogleUser {
            sub: raw.sub,
            primary_verified_email,
            name: raw.name,
            picture: raw.picture,
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
            ("grant_type", "authorization_code"),
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
                provider: "google".to_string(),
                status: status.as_u16(),
                message: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        let parsed: TokenResp = serde_json::from_slice(&bytes).context(JsonSnafu)?;
        if let Some(err) = parsed.error {
            return Err(Error::Api {
                provider: "google".to_string(),
                status: status.as_u16(),
                message: parsed.error_description.unwrap_or(err),
            });
        }
        parsed.access_token.context(ApiSnafu {
            provider: "google".to_string(),
            status: status.as_u16(),
            message: "token response missing access_token",
        })
    }

    async fn fetch_userinfo(
        &self,
        http: &reqwest::Client,
        access_token: &str,
    ) -> Result<RawUserInfo, Error> {
        let resp = http
            .get(USERINFO_URL)
            .bearer_auth(access_token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .context(HttpSnafu)?;
        let status = resp.status();
        let bytes = resp.bytes().await.context(HttpSnafu)?;
        if !status.is_success() {
            return Err(Error::Api {
                provider: "google".to_string(),
                status: status.as_u16(),
                message: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        serde_json::from_slice::<RawUserInfo>(&bytes).context(JsonSnafu)
    }
}

fn http_client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .context(HttpSnafu)
}

/// Google UserInfo endpoint 的 OIDC 标准响应字段。
/// 其它字段（`given_name`、`family_name`、`locale` 等）忽略不解析。
#[derive(Deserialize)]
struct RawUserInfo {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    picture: Option<String>,
}
