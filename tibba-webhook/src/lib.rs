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

//! 出站 webhook 投递：业务事件 → 用户配置的 URL，HMAC-SHA256 签名 + 失败重试 / 死信。
//!
//! 复用既有基础设施而非另起炉灶：
//! - **可靠投递**走 [`tibba_job`] 任务队列（自带指数退避重试 + 死信隔离）；
//! - **HTTP POST** 走 [`tibba_request`]（超时 + 通用日志 + 可选熔断器）；
//! - **签名**走 [`tibba_crypto::KeyGrip`]（HMAC-SHA256，天然支持密钥轮换）。
//!
//! ## 用法
//! 1）启动期注册 handler（须在 [`tibba_job::start`] 之前）：
//! ```ignore
//! let handler = WebhookHandler::builder().with_secret(secret).build()?;
//! tibba_job::register_handler(std::sync::Arc::new(handler));
//! ```
//! 2）业务事件触发投递：
//! ```ignore
//! enqueue(pool, &WebhookDelivery::new(url, "order.paid", payload)).await?;
//! ```
//! worker 取出后签名 + POST；2xx 视为成功，否则由任务队列重试，超过上限转死信。
//!
//! ## 接收方验签
//! 取 `X-Webhook-Timestamp` 头 `ts` 与请求体原始字节 `body`，拼出 `signed = ts + "." + body`，
//! 用约定 secret 计算 `hex(hmac_sha256(secret, signed))`，与 `X-Webhook-Signature` 头
//! （去掉 `sha256=` 前缀）做常量时间比较。签名已覆盖时间戳，接收方可再校验 `ts` 与当前时间
//! 的偏差（如 ±5 分钟）拒绝重放，并按 `X-Webhook-Id` 做幂等去重。

use http::Method;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use sqlx::PgPool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tibba_crypto::KeyGrip;
use tibba_error::Error as BaseError;
use tibba_job::{BoxFuture, Job, JobContext, JobHandler, JobQueue};
use tibba_request::{Client, ClientBuilder, Params};
use tracing::info;

/// 本 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:webhook=info`（或 `debug`）过滤。
const LOG_TARGET: &str = "tibba:webhook";

/// 任务类型名：出站 webhook。入队与 handler 须用同一常量，避免拼写漂移。
pub const JOB_WEBHOOK: &str = "webhook";

/// webhook 投递默认重试上限（短于通用任务的 25 次：持续投递不到的接收方尽早转死信）。
const DEFAULT_MAX_ATTEMPTS: i32 = 12;
/// 单次投递默认超时。
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// 签名头：值形如 `sha256=<hex(hmac_sha256(secret, body))>`。
const HEADER_SIGNATURE: &str = "x-webhook-signature";
/// 事件类型头，供接收方路由。
const HEADER_EVENT: &str = "x-webhook-event";
/// 投递唯一 id 头，供接收方幂等去重。
const HEADER_ID: &str = "x-webhook-id";
/// 投递时间戳头（unix 秒），供接收方防重放。
const HEADER_TIMESTAMP: &str = "x-webhook-timestamp";

type Result<T, E = Error> = std::result::Result<T, E>;

/// 本 crate 内部错误，统一通过 `From` 转换为 [`tibba_error::Error`]。
#[derive(Debug, Snafu)]
pub enum Error {
    /// payload 序列化 / 反序列化失败。
    #[snafu(display("webhook payload serde: {source}"))]
    Serde { source: serde_json::Error },

    /// 构造请求头时值非法（含非 ASCII / 控制字符）。
    #[snafu(display("invalid header value: {source}"))]
    HeaderValue {
        source: http::header::InvalidHeaderValue,
    },

    /// HMAC 签名失败（密钥初始化错误）。
    #[snafu(display("webhook sign: {source}"))]
    Sign { source: tibba_crypto::Error },

    /// 构造 webhook HTTP client 失败。
    #[snafu(display("build webhook client: {source}"))]
    BuildClient { source: tibba_request::Error },

    /// 投递失败（网络错误或非 2xx 响应）；触发任务队列重试。
    #[snafu(display("webhook delivery to {url} failed: {source}"))]
    Deliver {
        url: String,
        source: tibba_request::Error,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Serde { source } => BaseError::new(source).with_sub_category("serde"),
            Error::HeaderValue { source } => {
                BaseError::new(source).with_sub_category("header_value")
            }
            Error::Sign { source } => BaseError::new(source)
                .with_sub_category("sign")
                .with_exception(true),
            Error::BuildClient { source } => BaseError::new(source)
                .with_sub_category("build_client")
                .with_exception(true),
            Error::Deliver { url, source } => {
                BaseError::new(format!("delivery to {url} failed: {source}"))
                    .with_sub_category("deliver")
            }
        };
        err.with_category("webhook")
    }
}

/// 出站 webhook 投递描述。入队时序列化进 job payload，worker 取出后投递。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDelivery {
    /// 目标 URL（接收方 endpoint）。
    pub url: String,
    /// 事件类型，写入 `X-Webhook-Event` 头供接收方路由。
    pub event: String,
    /// 业务数据，作为 application/json 请求体发送，并参与签名。
    pub payload: serde_json::Value,
    /// 投递唯一 id（写入 `X-Webhook-Id` 供接收方幂等去重）；缺省自动生成。
    #[serde(default)]
    pub id: Option<String>,
}

impl WebhookDelivery {
    /// 新建投递：必填目标 URL、事件类型、业务数据。
    pub fn new(
        url: impl Into<String>,
        event: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            url: url.into(),
            event: event.into(),
            payload,
            id: None,
        }
    }

    /// 指定投递 id（缺省由 worker 投递时自动生成 uuid）。
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// 将一次 webhook 投递入队（事件触发入口）。返回新任务 id。
///
/// worker 异步投递；失败按任务队列策略指数退避重试，超过上限转死信。需在业务侧
/// 已注册 [`WebhookHandler`] 并启动 worker。如需与业务写库同事务，可改用
/// [`tibba_job::JobQueue::enqueue_tx`]（此处为便捷的非事务版本）。
pub async fn enqueue(
    pool: &'static PgPool,
    delivery: &WebhookDelivery,
) -> std::result::Result<i64, BaseError> {
    let payload = serde_json::to_value(delivery).context(SerdeSnafu)?;
    let job = Job::new(JOB_WEBHOOK, payload).with_max_attempts(DEFAULT_MAX_ATTEMPTS);
    JobQueue::new(pool).enqueue(&job).await
}

/// 出站 webhook 任务处理器：签名 + POST 投递。注册到 [`tibba_job`] 后处理 `webhook` 任务。
pub struct WebhookHandler {
    /// 复用 tibba-request 的 HTTP client（超时 + 通用拦截器 + 可选熔断器）。
    client: Client,
    /// 配置了签名密钥则对每次投递做 HMAC-SHA256 签名；`None` 则不签名。
    signer: Option<KeyGrip>,
}

impl WebhookHandler {
    /// 创建 builder。
    pub fn builder() -> WebhookHandlerBuilder {
        WebhookHandlerBuilder::new()
    }

    /// 执行一次投递：签名 + POST，2xx 视为成功（非 2xx 由通用拦截器转为 `Err` 触发重试）。
    /// `delivery_id` 由调用方传入且跨重试稳定，作为 `X-Webhook-Id` 供接收方幂等去重。
    async fn deliver(&self, delivery: &WebhookDelivery, delivery_id: &str) -> Result<()> {
        // 签名基于精确的 body 字节；reqwest 的 .json() 用同一 serde_json 序列化，字节一致，
        // 故此处对 to_vec 的结果签名，与下方 body 实际发送内容保持一致
        let body = serde_json::to_vec(&delivery.payload).context(SerdeSnafu)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let timestamp_str = timestamp.to_string();

        let mut headers = HeaderMap::with_capacity(4);
        insert_header(&mut headers, HEADER_EVENT, &delivery.event)?;
        insert_header(&mut headers, HEADER_ID, delivery_id)?;
        insert_header(&mut headers, HEADER_TIMESTAMP, &timestamp_str)?;
        if let Some(signer) = &self.signer {
            // 签名覆盖 `timestamp.body`（Stripe 式），把时间戳纳入签名：接收方无法在不失效
            // 签名的前提下改写 X-Webhook-Timestamp，据此可做重放窗口校验
            let mut signed_payload = Vec::with_capacity(timestamp_str.len() + 1 + body.len());
            signed_payload.extend_from_slice(timestamp_str.as_bytes());
            signed_payload.push(b'.');
            signed_payload.extend_from_slice(&body);
            let signature = signer.sign(&signed_payload).context(SignSnafu)?;
            insert_header(&mut headers, HEADER_SIGNATURE, &format!("sha256={signature}"))?;
        }

        self.client
            .request_raw(Params {
                method: Method::POST,
                timeout: None,
                url: &delivery.url,
                query: None::<&()>,
                body: Some(&delivery.payload),
                headers: Some(&headers),
            })
            .await
            .context(DeliverSnafu {
                url: delivery.url.clone(),
            })?;

        info!(
            target: LOG_TARGET,
            event = delivery.event,
            id = delivery_id,
            url = delivery.url,
            "webhook delivered"
        );
        Ok(())
    }
}

impl JobHandler for WebhookHandler {
    fn job_type(&self) -> &'static str {
        JOB_WEBHOOK
    }

    fn handle(&self, ctx: JobContext) -> BoxFuture<'_, std::result::Result<(), BaseError>> {
        Box::pin(async move {
            // job 行 id 跨重试稳定，作为默认投递 id；显式 with_id 时以显式值优先
            let job_id = ctx.id;
            let delivery: WebhookDelivery =
                serde_json::from_value(ctx.payload).context(SerdeSnafu)?;
            let delivery_id = delivery.id.clone().unwrap_or_else(|| job_id.to_string());
            self.deliver(&delivery, &delivery_id).await?;
            Ok(())
        })
    }
}

/// 把 `name: value` 写入 `headers`；值非法（非 ASCII / 控制字符）时返回错误。
fn insert_header(headers: &mut HeaderMap, name: &'static str, value: &str) -> Result<()> {
    let value = HeaderValue::from_str(value).context(HeaderValueSnafu)?;
    headers.insert(HeaderName::from_static(name), value);
    Ok(())
}

/// [`WebhookHandler`] 的 builder：无必填项，可选项走链式 `with_xxx`。
pub struct WebhookHandlerBuilder {
    secret: Option<String>,
    timeout: Duration,
    circuit_breaker: Option<(u32, Duration)>,
}

impl WebhookHandlerBuilder {
    fn new() -> Self {
        Self {
            secret: None,
            timeout: DEFAULT_TIMEOUT,
            circuit_breaker: None,
        }
    }

    /// 设置 HMAC-SHA256 签名密钥；空串视为未设置（不签名）。
    #[must_use]
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        let secret = secret.into();
        // 空密钥等同于不签名，避免用空 key 产出可被任意伪造的「签名」
        self.secret = if secret.is_empty() { None } else { Some(secret) };
        self
    }

    /// 设置单次投递超时（默认 10s）。
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 启用熔断器。
    ///
    /// 注意：熔断器作用于整个 webhook client（跨所有目标 URL 共享），多目标场景下
    /// 某个目标持续失败会拖累全部投递。故缺省关闭，由任务队列对**单条**投递做指数
    /// 退避重试来承担韧性；仅当所有 webhook 指向同一下游时才建议开启。
    #[must_use]
    pub fn with_circuit_breaker(mut self, threshold: u32, cooldown: Duration) -> Self {
        self.circuit_breaker = Some((threshold, cooldown));
        self
    }

    /// 构造 handler：内部建带超时 + 通用拦截器（状态码 ≥400 计失败 + 请求日志）的 client。
    pub fn build(self) -> Result<WebhookHandler> {
        let mut builder = ClientBuilder::new("webhook")
            .with_timeout(self.timeout)
            .with_common_interceptor();
        if let Some((threshold, cooldown)) = self.circuit_breaker {
            builder = builder.with_circuit_breaker(threshold, cooldown);
        }
        let client = builder.build().context(BuildClientSnafu)?;
        let signer = match self.secret {
            Some(secret) => Some(KeyGrip::new(vec![secret.into_bytes()]).context(SignSnafu)?),
            None => None,
        };
        Ok(WebhookHandler { client, signer })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// 入队 / 出队的 payload 契约：序列化为 Value 再还原应保持等价。
    #[test]
    fn delivery_payload_round_trips() {
        let delivery = WebhookDelivery::new(
            "https://example.com/hook",
            "order.paid",
            serde_json::json!({"amount": 100}),
        )
        .with_id("dlv_1");
        let value = serde_json::to_value(&delivery).unwrap();
        let back: WebhookDelivery = serde_json::from_value(value).unwrap();
        assert_eq!(back.url, "https://example.com/hook");
        assert_eq!(back.event, "order.paid");
        assert_eq!(back.id.as_deref(), Some("dlv_1"));
        assert_eq!(back.payload, serde_json::json!({"amount": 100}));
    }

    /// id 字段可缺省：接收 missing id 的 payload 也能反序列化（serde default）。
    #[test]
    fn id_is_optional_in_payload() {
        let value = serde_json::json!({"url": "u", "event": "e", "payload": {}});
        let delivery: WebhookDelivery = serde_json::from_value(value).unwrap();
        assert!(delivery.id.is_none());
    }

    /// 签名用 KeyGrip 产出 64 位 hex，且可用同一 KeyGrip 验签（接收方等价校验）。
    #[test]
    fn signature_is_hex_and_verifiable() {
        let kg = KeyGrip::new(vec![b"secret".to_vec()]).unwrap();
        let body = br#"{"amount":100}"#;
        let signature = kg.sign(body).unwrap();
        assert_eq!(signature.len(), 64);
        assert_eq!(kg.verify(body, &signature).unwrap(), (true, true));
    }

    /// 含非法字符的 header 值被拒绝（不会静默发出残缺头）。
    #[test]
    fn invalid_header_value_rejected() {
        let mut headers = HeaderMap::new();
        // 换行符在 header 值中非法
        assert!(insert_header(&mut headers, HEADER_EVENT, "bad\nvalue").is_err());
    }
}
