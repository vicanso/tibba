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
//! 基于 [`tibba_request`] 统一出站客户端（而未引入 `oauth2` crate），避免大依赖树并自动复用超时、OTel 与统计拦截器。
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
use std::fmt;
use tibba_error::Error as BaseError;

pub use github::{GitHubProvider, GitHubUser};
pub use google::{GoogleProvider, GoogleUser};

mod github;
mod google;

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:oauth=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:oauth";
pub(crate) const MAX_ERROR_BODY_BYTES: usize = 512;

/// 按字节上限截断 UTF-8 字符串，始终停在合法字符边界。
pub(crate) fn truncate_utf8_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let idx = s.floor_char_boundary(max_bytes);
    &s[..idx]
}

/// 按字节上限将响应体转为可安全放进错误信息的 UTF-8 文本。
pub(crate) fn format_error_body(bytes: &[u8]) -> String {
    let end = bytes.len().min(MAX_ERROR_BODY_BYTES);
    match std::str::from_utf8(&bytes[..end]) {
        Ok(s) => s.to_string(),
        Err(err) => String::from_utf8_lossy(&bytes[..err.valid_up_to()]).into_owned(),
    }
}

/// tibba-oauth 模块对外的错误类型。
#[derive(Debug, Snafu)]
pub enum Error {
    /// HTTP 调用失败（DNS / 超时 / 连接错误等）
    #[snafu(display("oauth http error: {source}"))]
    Http {
        #[snafu(source(from(tibba_request::Error, Box::new)))]
        source: Box<tibba_request::Error>,
    },
    /// 解析第三方响应 JSON 失败（响应格式与预期不符）
    #[snafu(display("oauth json parse error: {source}"))]
    Json { source: serde_json::Error },
    /// Token 交换被第三方拒绝或授权码无效/过期 → 401（客户端侧错误，不告警）
    #[snafu(display("oauth token rejected ({provider}): {message}"))]
    Rejected { provider: String, message: String },
    /// 第三方 API 返回异常状态或未返回预期用户身份
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
            // 复用 tibba_request::Error -> BaseError 转换，自动剥离 URL 中的敏感查询参数
            Error::Http { source } => {
                let mut err = BaseError::from(*source).with_sub_category("http");
                if err.status() == 500 {
                    err = err.with_status(502).with_exception(true);
                }
                err
            }
            Error::Json { source } => BaseError::new(source)
                .with_sub_category("json")
                .with_status(502)
                .with_exception(true),
            Error::Rejected { provider, message } => {
                let truncated = truncate_utf8_str(&message, MAX_ERROR_BODY_BYTES);
                BaseError::new(format!("{provider} oauth rejected: {truncated}"))
                    .with_sub_category("rejected")
                    .with_status(401)
                    .with_exception(false)
            }
            Error::Api {
                provider,
                status,
                message,
            } => {
                let truncated = truncate_utf8_str(&message, MAX_ERROR_BODY_BYTES);
                BaseError::new(format!("{provider} api error({status}): {truncated}"))
                    .with_sub_category("api")
                    .with_status(502)
                    .with_exception(status >= 500 || status == 429)
            }
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
#[derive(Clone, Default, Deserialize)]
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

impl fmt::Debug for GitHubConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubConfig")
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

impl GitHubConfig {
    /// 三个字段都非空才视为已配置。
    #[must_use]
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
#[derive(Clone, Default, Deserialize)]
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

impl fmt::Debug for GoogleConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleConfig")
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

impl GoogleConfig {
    /// 三个字段都非空才视为已配置。
    #[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_configs_and_providers_redact_client_secret_in_debug() {
        let gh_cfg = GitHubConfig {
            client_id: "gh-id".to_string(),
            client_secret: "gh-super-secret".to_string(),
            redirect_uri: "http://localhost/gh".to_string(),
        };
        let gh_dbg = format!("{gh_cfg:?}");
        assert!(!gh_dbg.contains("gh-super-secret"), "{gh_dbg}");
        assert!(gh_dbg.contains("[REDACTED]"), "{gh_dbg}");

        let gh_provider = gh_cfg.build_provider().unwrap();
        let gh_prov_dbg = format!("{gh_provider:?}");
        assert!(!gh_prov_dbg.contains("gh-super-secret"), "{gh_prov_dbg}");
        assert!(gh_prov_dbg.contains("[REDACTED]"), "{gh_prov_dbg}");

        let g_cfg = GoogleConfig {
            client_id: "google-id".to_string(),
            client_secret: "google-super-secret".to_string(),
            redirect_uri: "http://localhost/google".to_string(),
        };
        let g_dbg = format!("{g_cfg:?}");
        assert!(!g_dbg.contains("google-super-secret"), "{g_dbg}");
        assert!(g_dbg.contains("[REDACTED]"), "{g_dbg}");

        let g_provider = g_cfg.build_provider().unwrap();
        let g_prov_dbg = format!("{g_provider:?}");
        assert!(!g_prov_dbg.contains("google-super-secret"), "{g_prov_dbg}");
        assert!(g_prov_dbg.contains("[REDACTED]"), "{g_prov_dbg}");

        let oauth_cfg = OAuthConfig {
            github: gh_cfg,
            google: g_cfg,
        };
        let oauth_dbg = format!("{oauth_cfg:?}");
        assert!(!oauth_dbg.contains("gh-super-secret"));
        assert!(!oauth_dbg.contains("google-super-secret"));
    }

    #[test]
    fn error_mapping_and_utf8_truncation() {
        let rejected = BaseError::from(Error::Rejected {
            provider: "github".to_string(),
            message: "bad code".to_string(),
        });
        assert_eq!(rejected.category(), "oauth");
        assert_eq!(rejected.status(), 401);
        assert!(!rejected.is_exception());

        let not_cfg = BaseError::from(Error::NotConfigured {
            provider: "google".to_string(),
        });
        assert_eq!(not_cfg.status(), 503);
        assert!(!not_cfg.is_exception());

        let upstream_500 = BaseError::from(Error::Api {
            provider: "github".to_string(),
            status: 500,
            message: "你".repeat(300),
        });
        assert_eq!(upstream_500.status(), 502);
        assert!(upstream_500.is_exception());
        assert!(upstream_500.message().len() < 600);

        let upstream_400 = BaseError::from(Error::Api {
            provider: "google".to_string(),
            status: 400,
            message: "bad request".to_string(),
        });
        assert_eq!(upstream_400.status(), 502);
        assert!(!upstream_400.is_exception());

        // format_error_body 对半个多字节 UTF-8 截断不得产生替代字符
        let sample = "你好世界".repeat(100);
        let truncated = format_error_body(sample.as_bytes());
        assert!(truncated.len() <= MAX_ERROR_BODY_BYTES);
        assert!(!truncated.contains('\u{FFFD}'));
    }
}
