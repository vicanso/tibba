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

use crate::{Error, LOG_TARGET, ResendSnafu};
use resend_rs::Resend;
use resend_rs::types::CreateEmailBaseOptions;
use snafu::ResultExt;
use tibba_error::Error as BaseError;
use tracing::info;

type Result<T, E = BaseError> = std::result::Result<T, E>;

/// 应用级邮件发送服务。
///
/// 当前只有 Resend 一个 provider；后续要扩展（SMTP / SendGrid）可在内部 enum 分发，
/// 公共 API 不变。`api_key` / `from` 在构造时拷贝一次，发送时不再读取配置。
#[derive(Debug, Clone)]
pub struct EmailService {
    api_key: String,
    from: String,
    reply_to: Option<String>,
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

    /// 设置统一 Reply-To 地址，链式调用。
    #[must_use]
    pub fn with_reply_to(mut self, reply_to: impl Into<String>) -> Self {
        self.reply_to = Some(reply_to.into());
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

    async fn send(
        &self,
        to: &str,
        subject: &str,
        text: Option<&str>,
        html: Option<&str>,
    ) -> Result<()> {
        // 前置参数校验：service 自身配置
        if self.api_key.is_empty() {
            return Err(Error::Invalid {
                message: "email api_key not configured (set [email] api_key)".to_string(),
            }
            .into());
        }
        if self.from.is_empty() {
            return Err(Error::Invalid {
                message: "email from not configured (set [email] from)".to_string(),
            }
            .into());
        }
        // 前置参数校验：单次发送
        if to.is_empty() {
            return Err(Error::Invalid {
                message: "recipient is empty".to_string(),
            }
            .into());
        }
        if subject.is_empty() {
            return Err(Error::Invalid {
                message: "subject is empty".to_string(),
            }
            .into());
        }
        if text.unwrap_or("").is_empty() && html.unwrap_or("").is_empty() {
            return Err(Error::Invalid {
                message: "both text and html body are empty".to_string(),
            }
            .into());
        }

        let mut email = CreateEmailBaseOptions::new(&self.from, [to], subject);
        if let Some(t) = text.filter(|s| !s.is_empty()) {
            email = email.with_text(t);
        }
        if let Some(h) = html.filter(|s| !s.is_empty()) {
            email = email.with_html(h);
        }
        if let Some(reply_to) = &self.reply_to {
            email = email.with_reply(reply_to);
        }

        Resend::new(&self.api_key)
            .emails
            .send(email)
            .await
            .context(ResendSnafu)?;

        info!(target: LOG_TARGET, to, subject, "email sent");
        Ok(())
    }
}
