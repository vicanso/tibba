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

//! tibba-notify
//!
//! 统一通知 trait + 内置 Email / WeCom 两个实现 + 扇出 MultiNotifier。
//!
//! ## 设计要点
//! - `Notifier::send(&self, to: &str, msg: &NotifyMessage) -> Result<(), BaseError>`：
//!   单一抽象。`to` 是方法参数 —— 同一个 notifier 实例可发不同目标
//!   （e.g. 一个 `WecomRobotNotifier` 可发多个群机器人）
//! - `MultiNotifier`：扇出。**逐个 send，整体不 Err**——wecom 挂了不该影响 email；
//!   返回 `Vec<NotifyResult>` 让调用方逐项决定是否告警
//!
//! ## 用法
//!
//! ```ignore
//! use tibba_notify::{MultiNotifier, NotifyMessage, EmailNotifier, WecomRobotNotifier};
//!
//! let msg = NotifyMessage::new("镜像分析完成", "...");
//! let multi = MultiNotifier::new()
//!     .add(EmailNotifier::new(email_cfg.build_service()), "ops@example.com")
//!     .add(WecomRobotNotifier::new()?, "ROBOT_KEY_xxx");
//!
//! for r in multi.send_all(&msg).await {
//!     if !r.ok {
//!         tracing::warn!(kind = r.kind, target = %r.target, error = ?r.error, "notify failed");
//!     }
//! }
//! ```

use serde::Serialize;
use snafu::Snafu;
use tibba_email::EmailService;
use tibba_error::Error as BaseError;
use tracing::warn;

pub use wecom::WecomRobotNotifier;

mod wecom;

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:notify=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:notify";

/// tibba-notify 内部错误。Notifier `send` 直接返回 `tibba_error::Error`，
/// 这里只是给各 impl 自己捕获用。
#[derive(Debug, Snafu)]
pub enum Error {
    /// 构造 HTTP client 失败
    #[snafu(display("notify http client init: {source}"))]
    HttpInit {
        #[snafu(source(from(BaseError, Box::new)))]
        source: Box<BaseError>,
    },
    /// 通知参数非法（e.g. to 空、subject 空）
    #[snafu(display("invalid notify args: {message}"))]
    Invalid { message: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::HttpInit { source } => BaseError::new(*source).with_sub_category("http_init"),
            Error::Invalid { message } => BaseError::new(message)
                .with_sub_category("invalid")
                .with_status(400)
                .with_exception(false),
        };
        err.with_category("notify")
    }
}

/// 通知消息载荷。`subject` 给 Email subject / WeCom 第一行；`text` 是 markdown 正文；
/// `html` 仅 Email impl 用得上，WeCom 实现忽略。
#[derive(Debug, Clone, Serialize, Default)]
pub struct NotifyMessage {
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
}

impl NotifyMessage {
    pub fn new(subject: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            text: text.into(),
            html: None,
        }
    }

    /// 同时携带 HTML 版本（仅 Email impl 会使用）。
    #[must_use]
    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }
}

/// 通知通道抽象。所有 impl 必须 `Send + Sync` 以便挂在 MultiNotifier 里跨任务用。
///
/// 用 dyn trait object 支持 MultiNotifier 持有不同 provider 的混合列表，
/// 故 `send` 返回显式 `Pin<Box<dyn Future>>` 而非 `async fn`。
pub trait Notifier: Send + Sync {
    /// 发送通知。`to` 含义因 provider 而异：Email = 收件地址，WeCom = 机器人 key。
    fn send<'a>(
        &'a self,
        to: &'a str,
        msg: &'a NotifyMessage,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), BaseError>> + Send + 'a>>;

    /// 标识 provider 类型，用于日志 / NotifyResult 聚合。
    fn kind(&self) -> &'static str;
}

/// Email 通道实现——直接包 `EmailService`（同步配置 + Resend client）。
pub struct EmailNotifier {
    service: EmailService,
}

impl EmailNotifier {
    pub fn new(service: EmailService) -> Self {
        Self { service }
    }
}

impl Notifier for EmailNotifier {
    fn send<'a>(
        &'a self,
        to: &'a str,
        msg: &'a NotifyMessage,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), BaseError>> + Send + 'a>>
    {
        Box::pin(async move {
            // 有 HTML 走 multipart，否则纯文本——与 EmailService 已有方法对齐
            if let Some(html) = &msg.html {
                self.service
                    .send_multipart(to, &msg.subject, &msg.text, html)
                    .await
            } else {
                self.service.send_text(to, &msg.subject, &msg.text).await
            }
        })
    }

    fn kind(&self) -> &'static str {
        "email"
    }
}

/// MultiNotifier 单条扇出结果。`ok=false` 时 `error` 含错误消息（display 字符串）。
#[derive(Debug, Clone)]
pub struct NotifyResult {
    pub kind: &'static str,
    /// 当次发送的目标（Email 地址 / WeCom robot key 等）
    pub target: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// 扇出多通道发送。**整体不 Err**——逐个 send 后把每条结果都返回，
/// 调用方按需告警 / 重试，不让 wecom 挂了影响 email。
#[derive(Default)]
pub struct MultiNotifier {
    items: Vec<(Box<dyn Notifier>, String)>,
}

impl MultiNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一个通道。`to` 是该通道的目标地址（Email 邮箱 / WeCom 机器人 key 等）。
    #[must_use]
    pub fn add(mut self, notifier: impl Notifier + 'static, to: impl Into<String>) -> Self {
        self.items.push((Box::new(notifier), to.into()));
        self
    }

    /// 当前已挂载的通道数；空时 `send_all` 返回空 Vec。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `len() == 0`，便于调用方提前 short-circuit。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 逐个发送，收集所有结果。**永不 Err**——某通道异常仅作为该条 `NotifyResult.error`。
    /// 同时把失败行同步写入 warn 日志，调用方即便忽略返回值也能在日志里看到失败。
    pub async fn send_all(&self, msg: &NotifyMessage) -> Vec<NotifyResult> {
        let mut results = Vec::with_capacity(self.items.len());
        for (notifier, to) in &self.items {
            let kind = notifier.kind();
            let result = notifier.send(to, msg).await;
            let (ok, error) = match result {
                Ok(()) => (true, None),
                Err(e) => {
                    let s = e.to_string();
                    warn!(target: LOG_TARGET, kind, target = %to, error = %s, "notify failed");
                    (false, Some(s))
                }
            };
            results.push(NotifyResult {
                kind,
                target: to.clone(),
                ok,
                error,
            });
        }
        results
    }
}
