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

//! tibba-oauth
//!
//! 第三方身份提供商客户端。当前仅 GitHub；预留 `OAuthConfig` 扩展为多 provider。
//!
//! **手写 reqwest 而非用 `oauth2` crate**：GitHub OAuth 2.0 协议简单，避免引入大依赖树。
//!
//! ## 用法
//!
//! ```ignore
//! let cfg: OAuthConfig = app_config.sub_config("oauth").try_deserialize()?;
//! let github = cfg.github.build_provider()?;
//!
//! // start：拼 authorize URL，把用户重定向过去
//! let url = github.authorize_url("random-state-token");
//!
//! // callback：用 code 换 access_token 再拿用户信息
//! let user = github.exchange_and_fetch("code-from-callback").await?;
//! ```

use serde::Deserialize;
use snafu::Snafu;
use tibba_error::Error as BaseError;

pub use github::{GitHubProvider, GitHubUser};
pub use google::{GoogleProvider, GoogleUser};

mod github;
mod google;

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:oauth=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:oauth";

/// tibba-oauth 模块对外的错误类型。
#[derive(Debug, Snafu)]
pub enum Error {
    /// HTTP 调用失败（DNS / 超时 / 连接错误等）
    #[snafu(display("oauth http error: {source}"))]
    Http {
        #[snafu(source(from(reqwest::Error, Box::new)))]
        source: Box<reqwest::Error>,
    },
    /// 解析第三方响应 JSON 失败（响应格式与预期不符）
    #[snafu(display("oauth json parse error: {source}"))]
    Json { source: serde_json::Error },
    /// 第三方 API 返回 4xx / 5xx 或业务级错误（如 access_token 不存在）
    #[snafu(display("oauth api error ({provider} {status}): {message}"))]
    Api {
        provider: String,
        status: u16,
        message: String,
    },
    /// provider 配置缺失或非法（client_id / client_secret 等为空）
    #[snafu(display("oauth provider not configured: {provider}"))]
    NotConfigured { provider: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Http { source } => BaseError::new(source).with_sub_category("http"),
            Error::Json { source } => BaseError::new(source).with_sub_category("json"),
            Error::Api {
                provider,
                status,
                message,
            } => BaseError::new(format!("{provider} api error({status}): {message}"))
                .with_sub_category("api")
                .with_status(502),
            Error::NotConfigured { provider } => {
                BaseError::new(format!("oauth provider not configured: {provider}"))
                    .with_sub_category("not_configured")
                    .with_status(503)
                    .with_exception(false)
            }
        };
        err.with_category("oauth")
    }
}

/// 应用配置中的 `[oauth]` 段。预留多 provider 扩展位。
///
/// 整段 / 子段全用 `#[serde(default)]`，配置缺失时安全地走 Default（client_id 等空串）；
/// 真正发起 OAuth 时由各 provider 的 `build_provider()` 校验非空，缺失返回 `Error::NotConfigured`。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OAuthConfig {
    #[serde(default)]
    pub github: GitHubConfig,
    #[serde(default)]
    pub google: GoogleConfig,
}

/// GitHub OAuth App 配置。三个字段均通过 GitHub 开发者后台
/// （Settings → Developer settings → OAuth Apps）创建 App 时获得。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GitHubConfig {
    /// Client ID（公开值）
    #[serde(default)]
    pub client_id: String,
    /// Client Secret（机密，建议环境变量 `TIBBA_WEB__OAUTH__GITHUB__CLIENT_SECRET` 注入）
    #[serde(default)]
    pub client_secret: String,
    /// 回调地址，需在 GitHub App 后台「Authorization callback URL」一致
    /// 例如 `http://localhost:5000/api/oauth/github/callback`
    #[serde(default)]
    pub redirect_uri: String,
}

impl GitHubConfig {
    /// 三个字段都非空才视为已配置。
    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty()
            && !self.client_secret.is_empty()
            && !self.redirect_uri.is_empty()
    }

    /// 用本配置构造一个 GitHubProvider。
    /// 配置不完整时返回 `Error::NotConfigured`，handler 层据此回 503。
    pub fn build_provider(&self) -> Result<GitHubProvider, Error> {
        if !self.is_configured() {
            return Err(Error::NotConfigured {
                provider: "github".to_string(),
            });
        }
        Ok(GitHubProvider::new(
            self.client_id.clone(),
            self.client_secret.clone(),
            self.redirect_uri.clone(),
        ))
    }
}

/// Google OAuth App 配置。三个字段从 Google Cloud Console
/// （APIs & Services → Credentials → OAuth 2.0 Client IDs）创建 Web App 类型客户端获得。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GoogleConfig {
    /// Client ID，形如 `xxx.apps.googleusercontent.com`
    #[serde(default)]
    pub client_id: String,
    /// Client Secret（机密；推荐 `TIBBA_WEB__OAUTH__GOOGLE__CLIENT_SECRET` 环境变量注入）
    #[serde(default)]
    pub client_secret: String,
    /// 回调地址，需在 Google Cloud Console 客户端的「Authorized redirect URIs」列表内
    #[serde(default)]
    pub redirect_uri: String,
}

impl GoogleConfig {
    /// 三个字段都非空才视为已配置。
    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty()
            && !self.client_secret.is_empty()
            && !self.redirect_uri.is_empty()
    }

    /// 用本配置构造一个 GoogleProvider。
    pub fn build_provider(&self) -> Result<GoogleProvider, Error> {
        if !self.is_configured() {
            return Err(Error::NotConfigured {
                provider: "google".to_string(),
            });
        }
        Ok(GoogleProvider::new(
            self.client_id.clone(),
            self.client_secret.clone(),
            self.redirect_uri.clone(),
        ))
    }
}
