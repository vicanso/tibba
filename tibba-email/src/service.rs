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

use crate::{Error, LOG_TARGET};
use http::Method;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;
use tibba_error::Error as BaseError;
use tibba_request::{Client, ClientBuilder, Params};
use tracing::info;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const RESEND_EMAILS_URL: &str = "https://api.resend.com/emails";
const HTTP_TIMEOUT_SECS: u64 = 15;
const MAX_ERROR_BODY_BYTES: usize = 512;

static RESEND_CLIENT: OnceLock<Client> = OnceLock::new();

fn http_client() -> std::result::Result<Client, Error> {
    if let Some(client) = RESEND_CLIENT.get() {
        return Ok(client.clone());
    }
    let client = ClientBuilder::new("email-resend")
        .with_timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .with_common_interceptor()
        .build()
        .map_err(|e| Error::Send {
            message: BaseError::from(e).message().to_string(),
        })?;
    let _ = RESEND_CLIENT.set(client.clone());
    Ok(RESEND_CLIENT.get().cloned().unwrap_or(client))
}

#[derive(Serialize)]
struct ResendEmailPayload<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<&'a str>,
}

/// 应用级邮件发送服务。
///
/// 通过 [`tibba_request`] 直接调用 Resend REST API，复用了全站连接池、超时与 OTel 拦截器，
/// 避免引入 `resend-rs` 及其重复的 HTTP 依赖。手写 `Debug` 遮蔽 `api_key`。
#[derive(Clone)]
pub struct EmailService {
    api_key: String,
    from: String,
    reply_to: Option<String>,
}

impl fmt::Debug for EmailService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            ""
        } else {
            "[REDACTED]"
        };
        f.debug_struct("EmailService")
            .field("api_key", &api_key)
            .field("from", &self.from)
            .field("reply_to", &self.reply_to)
            .finish()
    }
}

fn contains_header_control_chars(s: &str) -> bool {
    s.contains(['\r', '\n', '\0'])
}

fn is_plausible_email_field(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() || contains_header_control_chars(trimmed) {
        return false;
    }
    // 支持纯邮箱 `a@b.c` 与带显示名格式 `"App <a@b.c>"`
    let addr = if let Some((_, inner)) = trimmed.rsplit_once('<')
        && let Some(addr) = inner.strip_suffix('>')
    {
        addr.trim()
    } else {
        trimmed
    };
    let Some((local, domain)) = addr.split_once('@') else {
        return false;
    };
    !local.trim().is_empty() && !domain.trim().is_empty() && !domain.contains('@')
}

fn truncate_utf8_prefix(bytes: &[u8], max_bytes: usize) -> String {
    let end = bytes.len().min(max_bytes);
    match std::str::from_utf8(&bytes[..end]) {
        Ok(s) => s.to_string(),
        Err(err) => {
            let valid_up_to = err.valid_up_to();
            String::from_utf8_lossy(&bytes[..valid_up_to]).into_owned()
        }
    }
}

impl EmailService {
    /// 用 Resend API key + 发件人地址构造一个 EmailService。
    /// `from` 必须是 Resend 后台已验证的域下邮箱（沙盒模式例外，详见 EmailConfig 文档）。
    pub fn new(api_key: impl Into<String>, from: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            from: from.into(),
            reply_to: None,
        }
    }

    /// 设置统一 Reply-To 地址，链式调用（空字符串自动忽略）。
    #[must_use]
    pub fn with_reply_to(mut self, reply_to: impl Into<String>) -> Self {
        let rt = reply_to.into();
        if rt.trim().is_empty() {
            self.reply_to = None;
        } else {
            self.reply_to = Some(rt);
        }
        self
    }

    /// 发送纯文本邮件。
    pub async fn send_text(&self, to: &str, subject: &str, text: &str) -> Result<()> {
        self.send(to, subject, Some(text), None).await
    }

    /// 发送 HTML 邮件。
    pub async fn send_html(&self, to: &str, subject: &str, html: &str) -> Result<()> {
        self.send(to, subject, None, Some(html)).await
    }

    /// 同时发送文本和 HTML 版本（推荐用法——多数客户端取 HTML，文本兜底）。
    pub async fn send_multipart(
        &self,
        to: &str,
        subject: &str,
        text: &str,
        html: &str,
    ) -> Result<()> {
        self.send(to, subject, Some(text), Some(html)).await
    }

    fn validate_send_args(
        &self,
        to: &str,
        subject: &str,
        text: Option<&str>,
        html: Option<&str>,
    ) -> std::result::Result<(), Error> {
        if self.api_key.trim().is_empty() {
            return Err(Error::NotConfigured {
                message: "email api_key not configured (set [email] api_key)".to_string(),
            });
        }
        if self.from.trim().is_empty() {
            return Err(Error::NotConfigured {
                message: "email from not configured (set [email] from)".to_string(),
            });
        }
        if !is_plausible_email_field(&self.from) {
            return Err(Error::Invalid {
                message: "invalid email from address".to_string(),
            });
        }
        if !is_plausible_email_field(to) {
            return Err(Error::Invalid {
                message: "invalid recipient email address".to_string(),
            });
        }
        if subject.trim().is_empty() || contains_header_control_chars(subject) {
            return Err(Error::Invalid {
                message: "invalid email subject".to_string(),
            });
        }
        if let Some(rt) = self
            .reply_to
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && !is_plausible_email_field(rt)
        {
            return Err(Error::Invalid {
                message: "invalid reply_to email address".to_string(),
            });
        }
        let has_text = text.is_some_and(|s| !s.is_empty());
        let has_html = html.is_some_and(|s| !s.is_empty());
        if !has_text && !has_html {
            return Err(Error::Invalid {
                message: "both text and html body are empty".to_string(),
            });
        }
        Ok(())
    }

    async fn send(
        &self,
        to: &str,
        subject: &str,
        text: Option<&str>,
        html: Option<&str>,
    ) -> Result<()> {
        self.validate_send_args(to, subject, text, html)?;

        let payload = ResendEmailPayload {
            from: self.from.trim(),
            to: [to.trim()],
            subject,
            text: text.filter(|s| !s.is_empty()),
            html: html.filter(|s| !s.is_empty()),
            reply_to: self
                .reply_to
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        };

        let client = http_client()?;
        let resp = client
            .execute(
                Params::new(Method::POST, RESEND_EMAILS_URL)
                    .with_bearer_auth(self.api_key.trim())
                    .with_body(&payload),
            )
            .await
            .map_err(|e| Error::Send {
                message: BaseError::from(e).message().to_string(),
            })?;

        if !resp.is_success() {
            #[derive(Deserialize)]
            struct ResendErrorBody {
                #[serde(default)]
                message: Option<String>,
            }
            let detail = serde_json::from_slice::<ResendErrorBody>(&resp.body)
                .ok()
                .and_then(|b| b.message)
                .unwrap_or_else(|| truncate_utf8_prefix(&resp.body, MAX_ERROR_BODY_BYTES));
            return Err(Error::Send {
                message: format!("resend api status {}: {detail}", resp.status),
            }
            .into());
        }

        info!(target: LOG_TARGET, to, subject, "email sent");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn service_debug_redacts_api_key() {
        let svc =
            EmailService::new("re_super_secret", "App <noreply@example.com>").with_reply_to("   ");
        assert_eq!(svc.reply_to, None);
        let dbg = format!("{svc:?}");
        assert!(!dbg.contains("re_super_secret"));
        assert!(dbg.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn unconfigured_returns_503_without_exception() {
        let svc = EmailService::new("", "noreply@example.com");
        let err = svc
            .send_text("u@example.com", "hi", "body")
            .await
            .expect_err("empty api_key should fail");
        assert_eq!(err.status(), 503);
        assert_eq!(err.sub_category(), Some("not_configured"));
        assert!(!err.is_exception());

        let svc = EmailService::new("re_123", "");
        let err = svc
            .send_text("u@example.com", "hi", "body")
            .await
            .expect_err("empty from should fail");
        assert_eq!(err.status(), 503);
    }

    #[tokio::test]
    async fn invalid_args_and_crlf_return_400() {
        let svc = EmailService::new("re_123", "noreply@example.com");
        for (to, subject, text) in [
            ("", "sub", "body"),
            ("not-an-email", "sub", "body"),
            ("a@b.com\r\nBcc:evil@x.com", "sub", "body"),
            ("a@b.com", "sub\nBcc:evil@x.com", "body"),
            ("a@b.com", "   ", "body"),
            ("a@b.com", "sub", ""),
        ] {
            let err = svc
                .send_text(to, subject, text)
                .await
                .expect_err("invalid email args should fail");
            assert_eq!(err.status(), 400, "failed for {to:?} / {subject:?}");
            assert!(!err.is_exception());
        }

        let bad_reply = svc.with_reply_to("bad\nheader@example.com");
        let err = bad_reply
            .send_text("a@b.com", "sub", "body")
            .await
            .expect_err("crlf reply_to should fail");
        assert_eq!(err.status(), 400);
    }

    #[test]
    fn send_error_maps_to_502_with_exception() {
        let err = BaseError::from(Error::Send {
            message: "upstream timeout".to_string(),
        });
        assert_eq!(err.category(), "email");
        assert_eq!(err.sub_category(), Some("send"));
        assert_eq!(err.status(), 502);
        assert!(err.is_exception());
    }
}
