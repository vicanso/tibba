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

//! WeCom（企业微信）群机器人 webhook 通知实现。
//!
//! 协议：`POST https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={robot_key}`
//! Body：`{ "msgtype": "markdown", "markdown": { "content": "..." } }`
//! Response：`{ "errcode": 0, "errmsg": "ok" }`
//!
//! ## 设计选择
//! - **复用 `tibba_request::ClientBuilder`**：拿到项目统一的拦截器 / 超时 / metrics
//! - **构造一次复用**：connection pool 在多次 send 间共享
//! - **`to` = robot_key**：同一 `WecomRobotNotifier` 实例可发不同群（Q2.A 选择）

use crate::{InvalidSnafu, LOG_TARGET, Notifier, NotifyMessage};
use serde::{Deserialize, Serialize};
use snafu::IntoError;
use std::time::Duration;
use tibba_error::Error as BaseError;
use tibba_request::ClientBuilder;
use tracing::debug;

const WEBHOOK_BASE: &str = "https://qyapi.weixin.qq.com/cgi-bin/webhook/send";

/// 企业微信群机器人通知实例。`to` = `key=...` query 里的机器人 key。
pub struct WecomRobotNotifier {
    client: tibba_request::Client,
}

impl WecomRobotNotifier {
    /// 构造一个 WeCom notifier。复用 `tibba_request::ClientBuilder("wecom")`，
    /// 自动套上项目级 metrics / 拦截器，超时 15s。
    pub fn new() -> Result<Self, BaseError> {
        let client = ClientBuilder::new("wecom")
            .with_timeout(Duration::from_secs(15))
            .with_common_interceptor()
            .build()?;
        Ok(Self { client })
    }
}

#[derive(Serialize)]
struct WecomPayload<'a> {
    msgtype: &'static str,
    markdown: WecomMarkdown<'a>,
}

#[derive(Serialize)]
struct WecomMarkdown<'a> {
    content: &'a str,
}

#[derive(Deserialize, Debug)]
struct WecomResp {
    #[serde(default)]
    errcode: i32,
    #[serde(default)]
    errmsg: String,
}

impl Notifier for WecomRobotNotifier {
    fn send<'a>(
        &'a self,
        to: &'a str,
        msg: &'a NotifyMessage,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), BaseError>> + Send + 'a>>
    {
        Box::pin(async move {
            // 前置校验：to 即 robot_key，不能空；subject + text 至少一个非空
            if to.is_empty() {
                return Err(InvalidSnafu {
                    message: "wecom robot_key is empty".to_string(),
                }
                .into_error(snafu::NoneError)
                .into());
            }
            if msg.subject.is_empty() && msg.text.is_empty() {
                return Err(InvalidSnafu {
                    message: "wecom message subject and text both empty".to_string(),
                }
                .into_error(snafu::NoneError)
                .into());
            }

            // WeCom markdown 消息：subject 作粗体首行，text 作正文
            let content = if msg.subject.is_empty() {
                msg.text.clone()
            } else if msg.text.is_empty() {
                format!("**{}**", msg.subject)
            } else {
                format!("**{}**\n\n{}", msg.subject, msg.text)
            };

            let url = format!("{WEBHOOK_BASE}?key={}", urlencode(to));
            let payload = WecomPayload {
                msgtype: "markdown",
                markdown: WecomMarkdown { content: &content },
            };

            let resp: WecomResp = self.client.post(&url, &payload).await?;

            // WeCom 的非 0 errcode 不会触发 HTTP 错误，得手动检查
            if resp.errcode != 0 {
                debug!(
                    target: LOG_TARGET,
                    errcode = resp.errcode,
                    errmsg = %resp.errmsg,
                    "wecom webhook business error"
                );
                return Err(InvalidSnafu {
                    message: format!("wecom errcode={} errmsg={}", resp.errcode, resp.errmsg),
                }
                .into_error(snafu::NoneError)
                .into());
            }
            Ok(())
        })
    }

    fn kind(&self) -> &'static str {
        "wecom"
    }
}

/// 最小 URL-encode：robot_key 通常 alphanumeric + 横杠/下划线，
/// 其它字节 percent-encode；不引入 urlencoding 直接依赖。
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
