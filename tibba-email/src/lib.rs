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

//! tibba-email
//!
//! 应用级邮件发送抽象。屏蔽具体提供商（当前仅 Resend），为业务侧提供
//! `EmailService` 句柄。配置由 `EmailConfig` 反序列化得到。
//!
//! ## 用法
//!
//! ```ignore
//! // 启动期初始化（src/config.rs 内）
//! let cfg: EmailConfig = app_config.sub_config("email").try_deserialize()?;
//! cfg.validate()?;
//!
//! // 业务侧使用
//! let svc = cfg.build_service();
//! svc.send_text("user@example.com", "Welcome", "Hi there!").await?;
//! ```

use serde::Deserialize;
use snafu::Snafu;
use tibba_error::Error as BaseError;

/// 本 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:email=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:email";

mod service;

pub use service::EmailService;

/// tibba-email 模块对外的错误类型。
#[derive(Debug, Snafu)]
pub enum Error {
    /// Resend HTTP/API 调用失败
    #[snafu(display("resend api error: {source}"))]
    Resend {
        // resend_rs::Error 内含 reqwest::Error，装箱避免 enum 膨胀
        #[snafu(source(from(resend_rs::Error, Box::new)))]
        source: Box<resend_rs::Error>,
    },
    /// 邮件参数非法（如收件人为空、subject 为空等）。
    /// 这是 EmailService 自身的前置校验，而非 Resend 的 API 错误。
    #[snafu(display("invalid email arguments: {message}"))]
    Invalid { message: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Resend { source } => BaseError::new(source).with_sub_category("resend"),
            Error::Invalid { message } => BaseError::new(message)
                .with_sub_category("invalid")
                .with_status(400)
                .with_exception(false),
        };
        err.with_category("email")
    }
}

/// 应用配置中的 `[email]` 节段。从 `tibba_config::Config` 反序列化。
///
/// 字段：
/// - `api_key`：Resend API key（生产环境强烈建议通过环境变量注入，避免落盘）
/// - `from`：发件人地址，需为 Resend 后台已验证的域名下的邮箱
///   （沙盒模式可用 `onboarding@resend.dev`，但收件人必须是账号绑定邮箱）
/// - `reply_to`：可选 Reply-To 地址
///
/// 所有字段都标了 `#[serde(default)]`，配置文件无 `[email]` 段或部分字段缺失时
/// 会得到空字符串/None，**应用照常启动**。真正发邮件时 `EmailService::send`
/// 会校验 `api_key` 和 `from` 非空，缺失则返回 `Error::Invalid`。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmailConfig {
    /// Resend API key，形如 `re_xxx`
    #[serde(default)]
    pub api_key: String,
    /// 发件人地址，例如 `"AppName <noreply@example.com>"` 或纯邮箱
    #[serde(default)]
    pub from: String,
    /// 可选 Reply-To 地址
    #[serde(default)]
    pub reply_to: Option<String>,
}

impl EmailConfig {
    /// 用本配置构造一个 `EmailService`。
    pub fn build_service(&self) -> EmailService {
        let mut svc = EmailService::new(self.api_key.clone(), self.from.clone());
        if let Some(reply_to) = &self.reply_to {
            svc = svc.with_reply_to(reply_to.clone());
        }
        svc
    }
}
