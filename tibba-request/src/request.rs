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

use super::{BuildSnafu, Error, LOG_TARGET, RequestSnafu, SerdeSnafu, UriSnafu};
use bytes::{Bytes, BytesMut};
use http::Method;
use http::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, LOCATION, RETRY_AFTER};
use reqwest::Client as ReqwestClient;
use reqwest::{RequestBuilder, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;
use snafu::ResultExt;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tibba_error::Error as BaseError;
use tibba_util::{Stopwatch, timestamp};
use tracing::{info, warn};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use url::Host;

type Result<T> = std::result::Result<T, Error>;

/// 装箱的异步 Future，用于 trait object 场景下的异步方法返回类型。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 把 OpenTelemetry 上下文写入 `HeaderMap` 的适配器，供全局 propagator 注入
/// W3C `traceparent` / `tracestate` 等追踪头。键 / 值非法时静默跳过（best-effort）。
struct HeaderInjector<'a>(&'a mut HeaderMap);

impl opentelemetry::propagation::Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(name) = HeaderName::from_bytes(key.as_bytes())
            && let Ok(val) = HeaderValue::from_str(&value)
        {
            self.0.insert(name, val);
        }
    }
}

/// crate 版本号，注入 User-Agent。
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 无查询参数 / 无请求体时的占位类型。
///
/// `Params` 的两个泛型必须有个具体类型才能推断，这个空切片类型就是它的默认值；
/// 调用方无需再手写 `None::<&[(&str, &str)]>` 之类的标注。
pub type NoParams = [(&'static str, &'static str)];

/// HTTP 请求参数，泛型 `Q` 为查询参数类型，`P` 为请求体类型，均须实现 `Serialize`。
///
/// 必填项（方法、URL）由 [`Params::new`] 接收，可选项通过链式 `with_xxx` 设置：
///
/// ```ignore
/// let params = Params::new(Method::POST, "/orders")
///     .with_body(&order)
///     .with_timeout(Duration::from_secs(5));
/// ```
///
/// 字段私有：此前是 6 个 `pub` 字段 + 结构体字面量构造，于是每个调用点都要写
/// 一遍 `timeout: None, query: None, headers: None`——四个可选项里通常只用到
/// 一个。链式写法同时也是本项目对「多可选参数结构体」的统一约定。
///
/// `with_query` / `with_body` 会改变对应的泛型参数，因此返回的是新类型。
/// 请求体承载方式：JSON 序列化、表单编码或原始字节（避免调用方预编码数据被二次 JSON 化）。
#[derive(Clone, Debug)]
enum BodyPayload<'a, P: Serialize + ?Sized> {
    Json(&'a P),
    Form(&'a P),
    Bytes {
        content_type: Option<&'static str>,
        data: &'a [u8],
    },
}

#[derive(Clone, Default)]
pub struct Params<'a, Q = NoParams, P = NoParams>
where
    Q: Serialize + ?Sized,
    P: Serialize + ?Sized,
{
    /// HTTP 方法
    pub(crate) method: Method,
    /// 单次请求超时，覆盖客户端默认值；`None` 则沿用客户端配置。
    timeout: Option<Duration>,
    /// URL 查询参数
    query: Option<&'a Q>,
    /// 请求体载荷（JSON / 表单 / 原始字节）
    body: Option<BodyPayload<'a, P>>,
    /// 请求 URL（绝对地址或相对于 base_url 的路径）
    url: &'a str,
    /// 单次请求附加头集合（如 webhook 签名、幂等键）；在拦截器之前应用，
    /// 仍可被后续拦截器覆盖。`None` 则不附加。
    headers: Option<&'a HeaderMap>,
    /// 通过 [`Params::with_header`] 逐个追加的单次请求头。
    extra_headers: HeaderMap,
    /// 单次请求 Bearer Token（在 Debug 输出中脱敏）。
    bearer_auth: Option<&'a str>,
}

impl<Q, P> fmt::Debug for Params<'_, Q, P>
where
    Q: Serialize + ?Sized,
    P: Serialize + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Params")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("timeout", &self.timeout)
            .field("has_query", &self.query.is_some())
            .field("has_body", &self.body.is_some())
            .field(
                "header_count",
                &(self.headers.map_or(0, HeaderMap::len) + self.extra_headers.len()),
            )
            .field("bearer_auth", &self.bearer_auth.map(|_| "[REDACTED]"))
            .finish()
    }
}

impl<'a> Params<'a> {
    /// 以 HTTP 方法与 URL 创建请求参数，其余项走链式方法设置。
    ///
    /// `url` 为 http(s) 绝对地址时直接使用，否则拼接客户端的 `base_url`。
    #[must_use]
    pub fn new(method: Method, url: &'a str) -> Self {
        Self {
            method,
            timeout: None,
            query: None,
            body: None,
            url,
            headers: None,
            extra_headers: HeaderMap::new(),
            bearer_auth: None,
        }
    }
}

impl<'a, Q, P> Params<'a, Q, P>
where
    Q: Serialize + ?Sized,
    P: Serialize + ?Sized,
{
    /// 设置本次请求的超时，覆盖客户端默认值，支持链式调用。
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// 设置本次请求附加的请求头集合，支持链式调用。
    #[must_use]
    pub fn with_headers(mut self, headers: &'a HeaderMap) -> Self {
        self.headers = Some(headers);
        self
    }

    /// 追加单个请求头（会在 `with_headers` 之后应用），支持链式调用。
    #[must_use]
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.extra_headers.insert(name, value);
        self
    }

    /// 设置本次请求的 Bearer Token（即 `Authorization: Bearer <token>`），支持链式调用。
    #[must_use]
    pub fn with_bearer_auth(mut self, token: &'a str) -> Self {
        self.bearer_auth = Some(token);
        self
    }

    /// 设置 URL 查询参数，支持链式调用。查询参数的类型由此确定。
    #[must_use]
    pub fn with_query<Q2>(self, query: &'a Q2) -> Params<'a, Q2, P>
    where
        Q2: Serialize + ?Sized,
    {
        Params {
            method: self.method,
            timeout: self.timeout,
            query: Some(query),
            body: self.body,
            url: self.url,
            headers: self.headers,
            extra_headers: self.extra_headers,
            bearer_auth: self.bearer_auth,
        }
    }

    /// 设置 JSON 请求体，支持链式调用。请求体的类型由此确定。
    #[must_use]
    pub fn with_body<P2>(self, body: &'a P2) -> Params<'a, Q, P2>
    where
        P2: Serialize + ?Sized,
    {
        Params {
            method: self.method,
            timeout: self.timeout,
            query: self.query,
            body: Some(BodyPayload::Json(body)),
            url: self.url,
            headers: self.headers,
            extra_headers: self.extra_headers,
            bearer_auth: self.bearer_auth,
        }
    }

    /// 设置 `application/x-www-form-urlencoded` 表单请求体，支持链式调用。
    #[must_use]
    pub fn with_form<P2>(self, form: &'a P2) -> Params<'a, Q, P2>
    where
        P2: Serialize + ?Sized,
    {
        Params {
            method: self.method,
            timeout: self.timeout,
            query: self.query,
            body: Some(BodyPayload::Form(form)),
            url: self.url,
            headers: self.headers,
            extra_headers: self.extra_headers,
            bearer_auth: self.bearer_auth,
        }
    }

    /// 设置原始字节请求体，不覆盖 `Content-Type`，支持链式调用。
    #[must_use]
    pub fn with_bytes(self, bytes: &'a [u8]) -> Params<'a, Q, NoParams> {
        Params {
            method: self.method,
            timeout: self.timeout,
            query: self.query,
            body: Some(BodyPayload::Bytes {
                content_type: None,
                data: bytes,
            }),
            url: self.url,
            headers: self.headers,
            extra_headers: self.extra_headers,
            bearer_auth: self.bearer_auth,
        }
    }

    /// 设置已预序列化的 JSON 字节请求体（自动写入 `Content-Type: application/json`），避免二次序列化。
    #[must_use]
    pub fn with_json_bytes(self, bytes: &'a [u8]) -> Params<'a, Q, NoParams> {
        Params {
            method: self.method,
            timeout: self.timeout,
            query: self.query,
            body: Some(BodyPayload::Bytes {
                content_type: Some("application/json"),
                data: bytes,
            }),
            url: self.url,
            headers: self.headers,
            extra_headers: self.extra_headers,
            bearer_auth: self.bearer_auth,
        }
    }
}

/// 对端 TLS 证书的有效期，用于证书临期告警。
///
/// 时间取 Unix 秒而非格式化字符串：本结构的消费者是 `on_done` 里的监控 / 告警逻辑，
/// 真正要算的是「还有几天过期」，整数直接相减即可，字符串反而要先解析回去。
/// 需要人类可读格式时由调用方自行格式化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TlsCertInfo {
    /// 证书生效时间（Unix 秒）
    pub not_before: i64,
    /// 证书过期时间（Unix 秒）
    pub not_after: i64,
}

impl TlsCertInfo {
    /// 距证书过期的剩余秒数；已过期返回负数。
    #[must_use]
    pub fn expires_in_secs(&self, now: i64) -> i64 {
        self.not_after - now
    }
}

/// 单次 HTTP 请求的性能统计，各时间字段单位为毫秒。
#[derive(Default, Clone, Debug)]
pub struct HttpStats {
    /// HTTP 方法
    pub method: String,
    /// 请求路径
    pub path: String,
    /// 服务端远端地址
    pub remote_addr: String,
    /// 响应状态码
    pub status: u16,
    /// 响应体字节数
    pub content_length: usize,
    /// 从发出请求到收到响应头的耗时（毫秒）
    pub processing: u32,
    /// 读取完整响应体的耗时（毫秒）
    pub transfer: u32,
    /// JSON 反序列化耗时（毫秒）
    pub serde: u32,
    /// 请求全程总耗时（毫秒）
    pub total: u32,
    /// 对端 TLS 证书有效期。`None` 表示无可用信息——未启用 `tls-info` feature、
    /// 请求走的是明文 HTTP、或证书解析失败，三种情况都归于此。
    ///
    /// 注：此前这里是 `tls_version` / `tls_not_before` / `tls_not_after` 三个
    /// `String`，但从未被填充过。`tls_version` 已删除且**无法**补上：reqwest 的
    /// `TlsInfo` 只暴露 `peer_certificate()`，协商的协议版本不对外提供。
    pub tls_cert: Option<TlsCertInfo>,
}

/// HTTP 请求拦截器 trait，用于在请求发出前后注入自定义逻辑（鉴权、日志、错误处理等）。
pub trait HttpInterceptor: Send + Sync {
    /// 响应状态码 ≥400 时调用，可将错误信息转换为业务 `Error`。
    fn fail(&self, _status: u16, _data: &Bytes) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    /// 发送前修改请求（如注入鉴权头、签名等）。
    fn request(&self, req: RequestBuilder) -> BoxFuture<'_, Result<RequestBuilder>> {
        Box::pin(async move { Ok(req) })
    }
    /// 收到响应体后进行转换（如解密、解包外层结构等）。
    fn response(&self, data: Bytes) -> BoxFuture<'_, Result<Bytes>> {
        Box::pin(async move { Ok(data) })
    }
    /// 请求完成后（无论成功或失败）的回调，可用于打印日志或上报指标。
    fn on_done(&self, _stats: &HttpStats, _err: Option<&Error>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// 跑完所有拦截器的 `on_done` 钩子；以 best-effort 方式处理钩子内部错误。
///
/// 之前 `on_done` 用 `?` 把拦截器错误向上传播，会覆盖原始请求结果：
/// 一个日志/统计回调失败就把成功的 HTTP 响应变成 Err，调用方拿到的
/// 是观测层的错误而不是真实业务结果。observability 的失败不应改写
/// business outcome——所以这里改成 warn! 记下来、继续跑下一个拦截器。
pub(crate) async fn run_on_done(config: &ClientConfig, stats: &HttpStats, err: Option<&Error>) {
    let Some(interceptors) = &config.interceptors else {
        return;
    };
    for interceptor in interceptors {
        if let Err(e) = interceptor.on_done(stats, err).await {
            warn!(
                target: LOG_TARGET,
                service = config.service,
                path = stats.path,
                error = %e,
                "on_done interceptor failed; original request result preserved",
            );
        }
    }
}

/// 状态码 ≥400 时，把上游响应体重建成结构化错误。
///
/// 此前这里是 `json_get(data, "message")`——只捞一个字符串，上游的
/// `category` / `sub_category` / `code` / `extra` 全部丢弃，且因为没设状态码，
/// 最终一律落地成本服务的 500 并被脱敏；运维在客户端侧只看得到
/// "internal server error"，连是哪个上游、回了什么都不知道。
///
/// 改走 [`BaseError::from_upstream`]：上游若是同样用 `tibba-error` 的服务，
/// 结构化字段原样带回；否则退化为截断后的响应体文本。对外状态码由
/// `From<Error> for BaseError` 统一映射成 502，见那里的说明。
pub fn handle_fail(service: &str, status: u16, data: &Bytes) -> Result<()> {
    if status >= 400 {
        return Err(Error::Upstream {
            service: service.to_string(),
            status,
            source: BaseError::from_upstream(status, data),
        });
    }
    Ok(())
}

/// 通用日志拦截器，请求完成后通过 tracing 记录详细统计信息。
pub struct CommonInterceptor {
    service: String,
}

impl CommonInterceptor {
    /// 以服务名创建通用拦截器实例。
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }
}

impl HttpInterceptor for CommonInterceptor {
    fn fail(&self, status: u16, data: &Bytes) -> BoxFuture<'_, Result<()>> {
        let result = handle_fail(&self.service, status, data);
        Box::pin(async move { result })
    }

    /// 请求完成后打印服务名、方法、路径、状态码、耗时等结构化日志。
    ///
    /// 日志**在返回 future 之前同步打完**，返回的是一个立即就绪的空 future。
    /// 此前是把 stats 的 7 个字段全 clone 进 `async move` 再 `Box::pin`——而块内
    /// 根本没有 await（`info!` 本就是同步的），那些 clone 纯粹是为了满足
    /// `BoxFuture` 的 `'static` 捕获要求，每请求白白分配 7 个 String。
    ///
    /// 需要真正异步上报（如把指标 POST 到外部服务）的实现，照常在 future 里做即可。
    fn on_done(&self, stats: &HttpStats, err: Option<&Error>) -> BoxFuture<'_, Result<()>> {
        // 证书剩余天数：直接给出可告警的数字，省得下游再算一遍。
        // `tls_cert` 为 None（未启用 tls-info / 明文 HTTP）时 tracing 不输出该字段，
        // 不给关闭 feature 的部署添噪音。
        let tls_expires_in_days = stats
            .tls_cert
            .map(|cert| cert.expires_in_secs(timestamp()) / 86_400);
        info!(
            target: LOG_TARGET,
            service = self.service,
            method = stats.method,
            path = stats.path,
            status = stats.status,
            remote_addr = stats.remote_addr,
            content_length = stats.content_length,
            processing = stats.processing,
            transfer = stats.transfer,
            serde = stats.serde,
            total = stats.total,
            tls_expires_in_days,
            error = err.map(ToString::to_string),
        );
        Box::pin(async { Ok(()) })
    }
}

/// 幂等方法可安全重复发送（GET/HEAD/PUT/DELETE/OPTIONS）；POST/PATCH 视为非幂等。
pub(crate) fn is_idempotent(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::PUT | Method::DELETE | Method::OPTIONS
    )
}

/// 网络层错误是否重试。连接未建立时服务端必然没收到，任何方法都可安全重试；
/// 超时 / 发送中断对非幂等方法不安全（可能已被处理），仅幂等方法重试。
fn should_retry_error(err: &reqwest::Error, idempotent: bool) -> bool {
    if err.is_connect() {
        true
    } else if err.is_timeout() || err.is_request() {
        idempotent
    } else {
        false
    }
}

/// 是否因响应状态码重试：仅对幂等方法，且状态为 429 或 5xx。
fn is_retryable_status(status: u16, idempotent: bool) -> bool {
    idempotent && (status == 429 || (500..=599).contains(&status))
}

/// 解析 DER 编码的证书取有效期；非法 DER 返回 `None`。
///
/// 与 [`extract_tls_cert`] 拆开是为了可测：`reqwest::tls::TlsInfo` 字段私有、
/// 无公开构造函数，没法在单测里伪造一个带证书的 `Response`，但 DER 解析这段
/// 可以拿真实证书直接验。
#[cfg(feature = "tls-info")]
fn parse_cert_validity(der: &[u8]) -> Option<TlsCertInfo> {
    let (_, cert) = x509_parser::parse_x509_certificate(der).ok()?;
    let validity = cert.validity();
    Some(TlsCertInfo {
        not_before: validity.not_before.timestamp(),
        not_after: validity.not_after.timestamp(),
    })
}

/// 从响应里取出对端证书有效期。
///
/// 全链路 best-effort：明文 HTTP（无 `TlsInfo`）、对端未送证书、DER 解析失败，
/// 一律返回 `None`。证书信息是观测数据，任何环节出问题都不该影响业务请求的结果。
#[cfg(feature = "tls-info")]
fn extract_tls_cert(res: &reqwest::Response) -> Option<TlsCertInfo> {
    let der = res
        .extensions()
        .get::<reqwest::tls::TlsInfo>()?
        .peer_certificate()?;
    parse_cert_validity(der)
}

/// SSRF 防护开启时是否应拦截该响应：3xx 一律拦截。
///
/// 重定向目标是响应回来才知道的，没有经过 `ensure_public_target`；跟随即绕过防护。
/// 未开启防护的客户端不受影响（reqwest 仍按默认策略自动跟随）。
fn is_blocked_redirect(deny_internal_targets: bool, status: u16) -> bool {
    deny_internal_targets && (300..400).contains(&status)
}

/// 指数退避的上界：`base * 2^attempt`，指数封顶 2^6（64 倍）防止过长等待。
fn retry_backoff_ceiling(base: Duration, attempt: u32) -> Duration {
    let factor = 1u32 << attempt.min(6);
    base.saturating_mul(factor)
}

/// 带抖动的指数退避：在 `[ceiling/2, ceiling)` 内随机取值。
///
/// 纯指数退避的问题是**所有**重试者算出同一个等待时长，于是在下游恢复的瞬间
/// 一起涌回去，把刚缓过来的服务再打垮（thundering herd）。加抖动把重试时刻摊开。
///
/// 取「等量抖动」而非 AWS 的 full jitter（`[0, ceiling)`）：保留一半固定退避，
/// 保证退避随重试次数单调增长，不会出现第 3 次重试反而比第 1 次等得更短。
fn retry_backoff(base: Duration, attempt: u32) -> Duration {
    let ceiling = retry_backoff_ceiling(base, attempt);
    let half = ceiling / 2;
    let spread = ceiling.saturating_sub(half);
    if spread.is_zero() {
        return ceiling;
    }
    half + Duration::from_nanos(rand::random_range(0..spread.as_nanos().max(1) as u64))
}

/// 简单熔断器：连续失败达 `threshold` 即打开，`cooldown` 内对请求快速失败；冷却结束后
/// 以**半开单探针**恢复（仅放行一个探测请求，成功则关闭、失败则重开）。状态用原子 + 极短
/// 临界区维护，**不跨 await 持锁**。
pub(crate) struct CircuitBreaker {
    /// 快速路径标志：关闭态（`false`）时 `allow()` 与 `record_success()` 无需获取互斥锁。
    is_open: AtomicBool,
    /// 连续失败计数。
    failures: AtomicU32,
    /// 打开阈值（连续失败次数）。
    threshold: u32,
    /// 打开后的冷却时长。
    cooldown: Duration,
    /// 打开截止时刻；`None` 表示关闭。临界区极短，不跨 await。
    open_until: Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            is_open: AtomicBool::new(false),
            failures: AtomicU32::new(0),
            threshold: threshold.max(1),
            cooldown,
            open_until: Mutex::new(None),
        }
    }

    /// 是否放行本次请求。处于打开 / 探测窗口内 → `false`；冷却结束 → 放行**单个**探测。
    ///
    /// 半开单探针：冷却结束后仅放行一个探测请求，并把窗口顺延一个 `cooldown`，使并发请求在
    /// 探测期间仍被拒（避免冷却到期时全部涌入的 thundering herd）。探测结果由
    /// `record_success`（关闭）/`record_failure`（重开）决定；探测若因异常未上报，窗口到期后
    /// 自然允许下一次探测，不会永久卡死。mutex 串行化保证同一时刻只放行一个探测。
    fn allow(&self) -> bool {
        if !self.is_open.load(Ordering::Acquire) {
            return true;
        }
        let mut guard = self.open_until.lock().unwrap_or_else(|e| e.into_inner());
        match *guard {
            Some(until) if Instant::now() < until => false,
            // 冷却结束：放行本次作为探测，窗口顺延一个 cooldown 以拒绝其余并发请求
            Some(_) => {
                *guard = Some(Instant::now() + self.cooldown);
                true
            }
            None => true,
        }
    }

    /// 记一次成功：清零失败计数并关闭熔断。
    pub(crate) fn record_success(&self) {
        if self.failures.load(Ordering::Acquire) != 0 {
            self.failures.store(0, Ordering::Release);
        }
        if !self.is_open.load(Ordering::Acquire) {
            return;
        }
        let mut guard = self.open_until.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
        self.is_open.store(false, Ordering::Release);
    }

    /// 记一次失败：达阈值则打开熔断（设置冷却截止时刻）。
    pub(crate) fn record_failure(&self) {
        let failures = self.failures.fetch_add(1, Ordering::AcqRel) + 1;
        if failures >= self.threshold {
            let until = Instant::now() + self.cooldown;
            let mut guard = self.open_until.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(until);
            self.is_open.store(true, Ordering::Release);
        }
    }
}

/// HTTP 客户端内部配置，由 `ClientBuilder` 填充后转移给 `Client`。
pub(crate) struct ClientConfig {
    /// 服务名称，用于日志和错误标识
    pub(crate) service: String,
    /// 所有相对路径请求的基础 URL
    base_url: String,
    /// 读取响应体的超时时间
    read_timeout: Option<Duration>,
    /// 整体请求超时时间（含连接 + 传输）
    timeout: Option<Duration>,
    /// TCP 连接超时时间
    connect_timeout: Option<Duration>,
    /// 连接池空闲超时时间
    pool_idle_timeout: Option<Duration>,
    /// 每个 host 最大空闲连接数，0 表示使用默认值
    pool_max_idle_per_host: usize,
    /// 最大并发在途请求数，超出时返回 `Error::ConcurrencyLimit`
    max_processing: Option<u32>,
    /// 每个请求都附带的默认请求头
    headers: Option<HeaderMap>,
    /// 自定义 DNS 解析映射，用于测试或内网转发
    dns_overrides: Option<HashMap<String, Vec<SocketAddr>>>,
    /// 请求拦截器链，按注册顺序依次执行
    interceptors: Option<Vec<Box<dyn HttpInterceptor>>>,
    /// 最大重试次数（0 = 不重试，保持原有行为）
    max_retries: u32,
    /// 重试退避基数（按指数增长）
    retry_base_delay: Duration,
    /// 可选熔断器：连续失败达阈值后在冷却期内快速失败
    pub(crate) circuit_breaker: Option<CircuitBreaker>,
    /// 开启后拒绝目标解析到内部地址（私网 / 回环 / 链路本地 / 云元数据）的请求，防 SSRF。
    /// 对投递到用户 / 运维可控 URL 的客户端（webhook、探测）应开启。
    deny_internal_targets: bool,
    /// 响应体字节上限；`None` = 不限制（仅在调用方显式关闭时出现）
    max_response_bytes: Option<usize>,
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 默认请求头常携带 Authorization / x-api-key 等凭证，仅输出头部数量不暴露值
        f.debug_struct("ClientConfig")
            .field("service", &self.service)
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .field("read_timeout", &self.read_timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("pool_idle_timeout", &self.pool_idle_timeout)
            .field("pool_max_idle_per_host", &self.pool_max_idle_per_host)
            .field("max_processing", &self.max_processing)
            .field(
                "header_count",
                &self.headers.as_ref().map_or(0, HeaderMap::len),
            )
            .field(
                "interceptor_count",
                &self.interceptors.as_ref().map_or(0, Vec::len),
            )
            .field("max_retries", &self.max_retries)
            .field("retry_base_delay", &self.retry_base_delay)
            .field("has_circuit_breaker", &self.circuit_breaker.is_some())
            .field("deny_internal_targets", &self.deny_internal_targets)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish()
    }
}

/// 默认整体请求超时（含建连 + 传输）。未调用 [`ClientBuilder::with_timeout`] 时生效。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// 默认 TCP 连接超时。
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 默认重试退避基数（仅在 `with_retry` 开启后使用）。
pub const DEFAULT_RETRY_BASE_DELAY: Duration = Duration::from_millis(100);

/// 默认响应体上限：64 MiB（与 `tibba_util::DEFAULT_DECOMPRESS_LIMIT` 同量级）。
///
/// # 为什么必须有默认值
/// `reqwest` 不限制响应体大小，`Response::bytes()` 会把对端发来的一切读进内存。
/// 本 crate 的主要用途之一是向**用户 / 运维可控的 URL** 发请求（webhook 投递、
/// 外部探测）——对端只要回一个几 GB 的流就能把进程撑爆，而 30s 的整体超时在
/// 内网带宽下根本拦不住。
///
/// 上限只能是**默认开启**：指望每个调用点都记得自己配一个，等于没有。64 MiB
/// 对任何 JSON API 响应都绰绰有余；确有大响应的场景用
/// [`ClientBuilder::with_max_response_bytes`] 显式调整。
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

/// 尊重 `Retry-After` 时允许的最长等待。
///
/// 上限必不可少：`Retry-After` 完全由对端指定，一个写着 `86400` 的响应会把
/// 我们的任务挂起一整天。超过上限时退回本地退避算法。
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// 解析 `Retry-After` 头，仅接受 delta-seconds 形式。
///
/// 规范还允许 HTTP-date，但实践中限流响应几乎都用秒数；为一个边缘形态引入
/// 日期解析依赖不划算，解析不出来就退回本地退避（只是等得久一点，不影响正确性）。
///
/// 超过 [`MAX_RETRY_AFTER`] 的值视为不可信，返回 `None`。
fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let secs: u64 = headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let delay = Duration::from_secs(secs);
    (delay <= MAX_RETRY_AFTER).then_some(delay)
}

/// HTTP 客户端构建器，通过链式调用配置后调用 `.build()` 生成 `Client`。
///
/// ## 默认值
/// | 项 | 默认 | 说明 |
/// |----|------|------|
/// | `timeout` | 30s | 整体请求超时 |
/// | `connect_timeout` | 5s | TCP 建连 |
/// | `max_retries` | 0 | 默认不重试 |
/// | `retry_base_delay` | 100ms | 指数退避基数，封顶 64× |
///
/// ## 重试语义
/// 仅当 `with_retry(n, …)` 且 `n > 0` 时重试：
/// - **幂等方法**（GET/HEAD/PUT/DELETE/OPTIONS）：网络超时、连接失败、429、5xx
/// - **非幂等**（POST/PATCH）：仅**连接未建立**时重试（服务端必未收到）
/// - 超时 / 发送中断对非幂等**不**重试（可能已处理）
#[derive(Debug)]
pub struct ClientBuilder {
    config: ClientConfig,
}

impl ClientBuilder {
    /// 以服务名创建构建器；超时类选项带安全默认值（见模块常量）。
    pub fn new(service: &str) -> Self {
        Self {
            config: ClientConfig {
                service: service.to_string(),
                base_url: String::new(),
                read_timeout: None,
                timeout: Some(DEFAULT_TIMEOUT),
                connect_timeout: Some(DEFAULT_CONNECT_TIMEOUT),
                pool_idle_timeout: None,
                pool_max_idle_per_host: 0,
                headers: None,
                interceptors: None,
                max_processing: None,
                dns_overrides: None,
                max_retries: 0,
                retry_base_delay: DEFAULT_RETRY_BASE_DELAY,
                circuit_breaker: None,
                deny_internal_targets: false,
                max_response_bytes: Some(DEFAULT_MAX_RESPONSE_BYTES),
            },
        }
    }

    /// 设置基础 URL，相对路径请求将拼接在此 URL 之后。
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.config.base_url = base_url.into();
        self
    }

    /// 开启 SSRF 防护：拒绝目标解析到内部地址（私网 / 回环 / 链路本地 / 云元数据）的请求。
    /// 用于向用户 / 运维可控 URL 发请求的客户端（webhook 投递、外部探测等）。
    ///
    /// # 副作用：不再跟随重定向
    /// 开启后本客户端一律**不跟随 3xx**，收到重定向直接返回 [`Error::BlockedRedirect`]。
    ///
    /// 这不是可选的加固，而是防护成立的前提：目标校验发生在发请求**之前**，而
    /// 重定向目标是响应回来才知道的。若仍按 reqwest 默认自动跟随，攻击者只需让
    /// 自己的公网域名回一个 `302 Location: http://169.254.169.254/…`，就能把校验
    /// 整个绕过去——防护形同虚设。
    ///
    /// 代价是「目标端点靠 3xx 跳转」的场景会失败。这类场景应直接配置最终 URL；
    /// 若确实需要跟随，正确做法是逐跳复跑 `ensure_public_target`，而不是放开策略。
    #[must_use]
    pub fn with_deny_internal_targets(mut self) -> Self {
        self.config.deny_internal_targets = true;
        self
    }

    /// 追加一个请求拦截器，拦截器按注册顺序链式执行。
    #[must_use]
    pub fn with_interceptor(mut self, interceptor: Box<dyn HttpInterceptor>) -> Self {
        self.config
            .interceptors
            .get_or_insert_with(Vec::new)
            .push(interceptor);
        self
    }

    /// 设置整体请求超时时间（含建连和传输）。
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = Some(timeout);
        self
    }

    /// 关闭整体请求超时（适用于大模型长连接 SSE 流式响应等场景，建议配合 `read_timeout` 或上层 deadline）。
    #[must_use]
    pub fn without_timeout(mut self) -> Self {
        self.config.timeout = None;
        self
    }

    /// 设置响应体读取超时时间。
    #[must_use]
    pub fn with_read_timeout(mut self, read_timeout: Duration) -> Self {
        self.config.read_timeout = Some(read_timeout);
        self
    }

    /// 设置 TCP 连接超时时间。
    #[must_use]
    pub fn with_connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.config.connect_timeout = Some(connect_timeout);
        self
    }

    /// 设置连接池空闲连接的回收超时时间。
    #[must_use]
    pub fn with_pool_idle_timeout(mut self, pool_idle_timeout: Duration) -> Self {
        self.config.pool_idle_timeout = Some(pool_idle_timeout);
        self
    }

    /// 设置每个请求默认携带的请求头。
    #[must_use]
    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.config.headers = Some(headers);
        self
    }

    /// 追加单个默认请求头（在每个请求上自动携带）。
    #[must_use]
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.config
            .headers
            .get_or_insert_with(HeaderMap::new)
            .insert(name, value);
        self
    }

    /// 追加通用日志拦截器（`CommonInterceptor`），自动使用当前服务名。
    #[must_use]
    pub fn with_common_interceptor(self) -> Self {
        let service = self.config.service.clone();
        self.with_interceptor(Box::new(CommonInterceptor::new(&service)))
    }

    /// 设置每个 host 的最大空闲连接数。
    #[must_use]
    pub fn with_pool_max_idle_per_host(mut self, pool_max_idle_per_host: usize) -> Self {
        self.config.pool_max_idle_per_host = pool_max_idle_per_host;
        self
    }

    /// 设置最大并发在途请求数，超出时立即返回错误，防止雪崩。
    #[must_use]
    pub fn with_max_processing(mut self, max_processing: u32) -> Self {
        self.config.max_processing = Some(max_processing);
        self
    }

    /// 设置自定义 DNS 解析映射，格式为 `host -> [SocketAddr]`。
    #[must_use]
    pub fn with_dns_overrides(mut self, dns_overrides: HashMap<String, Vec<SocketAddr>>) -> Self {
        self.config.dns_overrides = Some(dns_overrides);
        self
    }

    /// 启用自动重试：`max_retries` 为最大重试次数，`base_delay` 为退避基数（指数增长）。
    /// 仅幂等方法按状态码 / 超时重试；非幂等方法仅在「连接未建立」时重试。默认不重试。
    ///
    /// # 超时是**每次尝试**的，不是整体
    /// [`Self::with_timeout`] 设的是单次请求的超时。开启重试后，最坏耗时约为
    /// `(max_retries + 1) × timeout + 各次退避之和`——`with_retry(3, …)` 配
    /// 30s 超时意味着这个调用最久可能占用两分钟。若调用方自身有 SLA，应当把
    /// 单次 `timeout` 相应调小，或在外层再包一个整体 deadline。
    ///
    /// 退避时长优先采用响应里的 `Retry-After`（仅 delta-seconds 形式，且不超过
    /// 60s），否则用带抖动的指数退避。
    #[must_use]
    pub fn with_retry(mut self, max_retries: u32, base_delay: Duration) -> Self {
        self.config.max_retries = max_retries;
        self.config.retry_base_delay = base_delay;
        self
    }

    /// 设置响应体字节上限，超出即中止读取并返回 [`Error::ResponseTooLarge`]。
    ///
    /// 默认 [`DEFAULT_MAX_RESPONSE_BYTES`]（64 MiB），见其文档说明为何默认开启。
    #[must_use]
    pub fn with_max_response_bytes(mut self, max_response_bytes: usize) -> Self {
        self.config.max_response_bytes = Some(max_response_bytes);
        self
    }

    /// 关闭响应体大小限制。
    ///
    /// **仅用于确实需要拉取超大响应、且对端完全可信的场景**。对端不可信时
    /// 这等于把进程内存交给对方支配，见 [`DEFAULT_MAX_RESPONSE_BYTES`]。
    #[must_use]
    pub fn without_response_limit(mut self) -> Self {
        self.config.max_response_bytes = None;
        self
    }

    /// 启用熔断：连续失败达 `threshold` 次后打开，`cooldown` 内对请求快速失败。默认关闭。
    #[must_use]
    pub fn with_circuit_breaker(mut self, threshold: u32, cooldown: Duration) -> Self {
        self.config.circuit_breaker = Some(CircuitBreaker::new(threshold, cooldown));
        self
    }

    /// 根据当前配置构建 `Client` 实例。
    pub fn build(mut self) -> Result<Client> {
        let mut builder = ReqwestClient::builder()
            .user_agent(format!("tibba-request/{VERSION}"))
            .referer(false);
        if let Some(timeout) = self.config.timeout {
            builder = builder.timeout(timeout);
        }
        if let Some(headers) = self.config.headers.take() {
            builder = builder.default_headers(headers);
        }
        if let Some(read_timeout) = self.config.read_timeout {
            builder = builder.read_timeout(read_timeout);
        }
        if let Some(connect_timeout) = self.config.connect_timeout {
            builder = builder.connect_timeout(connect_timeout);
        }
        if let Some(pool_idle_timeout) = self.config.pool_idle_timeout {
            builder = builder.pool_idle_timeout(pool_idle_timeout);
        }
        if self.config.pool_max_idle_per_host > 0 {
            builder = builder.pool_max_idle_per_host(self.config.pool_max_idle_per_host);
        }
        if let Some(dns_overrides) = self.config.dns_overrides.take() {
            for (host, addrs) in dns_overrides {
                if addrs.is_empty() {
                    return Err(Error::Common {
                        service: self.config.service.clone(),
                        message: format!("empty dns override for host: {host}"),
                    });
                }
                if self.config.deny_internal_targets
                    && addrs.iter().any(|addr| is_internal_ip(addr.ip()))
                {
                    return Err(Error::BlockedTarget {
                        service: self.config.service.clone(),
                        host,
                    });
                }
                builder = builder.resolve_to_addrs(&host, &addrs);
            }
        }
        // TLS 信息采集只在 `tls-info` feature 下开启：它会让 reqwest 把对端证书
        // DER 复制进每个响应的 extensions，是每响应一次的堆分配。此前无条件开启
        // 却从无人读取，纯属白付成本。
        #[cfg(feature = "tls-info")]
        {
            builder = builder.tls_info(true);
        }
        // SSRF 防护开启时必须关掉自动跟随与系统代理：
        // - reqwest 默认最多跟 10 跳，跟随未经校验的 Location 等于绕过校验；
        // - 禁止环境代理防止通过 HTTP_PROXY/HTTPS_PROXY 间接访问内网资源。
        if self.config.deny_internal_targets {
            builder = builder
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .dns_resolver(Arc::new(PublicOnlyResolver));
        }

        let client = builder.build().context(BuildSnafu {
            service: self.config.service.clone(),
        })?;
        Ok(Client {
            client,
            config: Arc::new(self.config),
            processing: Arc::new(AtomicU32::new(0)),
        })
    }
}

/// 在途请求计数守卫，释放时自动归还并发配额，亦可转入流式响应生命周期持有。
pub(crate) struct ProcessingGuard {
    counter: Arc<AtomicU32>,
}

impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Release);
    }
}

/// `Client::execute` 返回的原始 HTTP 响应（状态码、响应头与完整响应体字节）。
///
/// 供需要自行判定非 2xx 状态码或读取响应头的调用方（如 OAuth、LLM 等）使用。
#[derive(Debug, Clone)]
pub struct RawResponse {
    /// 上游返回的 HTTP 状态码。
    pub status: u16,
    /// 上游返回的响应头。
    pub headers: HeaderMap,
    /// 经大小限制读取的完整响应体字节。
    pub body: Bytes,
}

impl RawResponse {
    /// 状态码是否为 2xx 成功。
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// HTTP 客户端，封装 reqwest `Client`，提供带拦截器链和并发限制的请求方法。
#[derive(Clone)]
pub struct Client {
    /// 底层 reqwest 客户端（内部已由 Arc 管理连接池，Clone 开销极低）
    client: ReqwestClient,
    /// 客户端配置（服务名、超时、拦截器等）
    pub(crate) config: Arc<ClientConfig>,
    /// 当前在途请求数，用于并发限制
    processing: Arc<AtomicU32>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("config", &self.config)
            .field("processing", &self.processing.load(Ordering::Relaxed))
            .finish()
    }
}

/// 判断 IP 是否为内部 / 特殊地址，SSRF 场景下应拒绝：
/// 回环、私网、链路本地（含 169.254.169.254 云元数据）、CGNAT、文档段、未指定 / 广播，
/// 以及 IPv6 的 ULA / link-local / IPv4-mapped 内部地址。
fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_internal_ipv4(v4),
        IpAddr::V6(v6) => {
            // 各种「IPv6 里夹带 IPv4」的形式都按内嵌的 v4 判断，否则攻击者换一种
            // 写法就能绕过 v4 的规则：
            // - IPv4-mapped ::ffff:a.b.c.d
            // - NAT64 RFC 6052 64:ff9b::a.b.c.d：经 NAT64 网关会被翻译成 a.b.c.d
            // - 6to4 2002:AABB:CCDD::/48：内嵌 v4 AA.BB.CC.DD
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_internal_ipv4(v4);
            }
            let seg = v6.segments();
            if seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return is_internal_ipv4(embedded_v4(seg[6], seg[7]));
            }
            if seg[0] == 0x2002 {
                return is_internal_ipv4(embedded_v4(seg[1], seg[2]));
            }
            let first = seg[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // ULA fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (first & 0xffc0) == 0xfec0 // site-local fec0::/10（已废弃，但仍可能路由到内网）
                // NAT64 本地使用前缀 64:ff9b:1::/48（RFC 8215）
                || seg[..3] == [0x0064, 0xff9b, 0x0001]
                // 丢弃专用前缀 100::/64（RFC 6666）
                || seg[..4] == [0x0100, 0, 0, 0]
                // Teredo 隧道前缀 2001::/32（RFC 4380）
                || (first == 0x2001 && seg[1] == 0)
                // 基准测试前缀 2001:2::/48（RFC 5180）
                || (first == 0x2001 && seg[1] == 0x0002 && seg[2] == 0)
                // 文档示例前缀 2001:db8::/32（RFC 3849）与 3fff::/20（RFC 9637）
                || (first == 0x2001 && seg[1] == 0x0db8)
                || (first == 0x3fff && (seg[1] & 0xf000) == 0)
                // IPv4-compatible ::a.b.c.d（已废弃）：前 96 位全 0。::1 / :: 已在上面处理
                || seg[..6] == [0; 6]
        }
    }
}

/// 两个 16 位段拼成一个 IPv4 地址。
fn embedded_v4(hi: u16, lo: u16) -> std::net::Ipv4Addr {
    let [a, b] = hi.to_be_bytes();
    let [c, d] = lo.to_be_bytes();
    std::net::Ipv4Addr::new(a, b, c, d)
}

/// IPv4 是否为内部 / 特殊地址。
///
/// 除标准库已有的判定外，补上标准库尚未稳定（或没有）的几段：
/// - `0.0.0.0/8`：「本网络」。Linux 上连 `0.0.0.0` 等价于连本机，
///   `is_unspecified` 只认恰好 `0.0.0.0`，`0.1.2.3` 之类会漏掉
/// - `100.64.0.0/10`：运营商级 NAT 共享地址
/// - `192.0.0.0/24`：IETF 协议分配
/// - `198.18.0.0/15`：基准测试网段，常被内网拿来用
/// - `224.0.0.0/4` 组播、`240.0.0.0/4` 保留（含广播）
fn is_internal_ipv4(v4: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_unspecified()
        || v4.is_documentation()
        || v4.is_multicast()
        || a == 0
        || (a == 100 && (b & 0xc0) == 64)
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b & 0xfe) == 18)
        || a >= 240
}

/// 开启 SSRF 防护时挂到 reqwest 上的 DNS 解析器：解析结果里只要出现内部地址就拒绝。
///
/// # 为什么必须在解析器里做
/// 此前的做法是发请求前自己 `lookup_host` 一次、校验通过后交给 reqwest——而
/// reqwest 建连时会**再解析一次**。攻击者控制的域名第一次回公网地址通过校验，
/// 第二次回 `169.254.169.254`，校验就被绕过了（DNS rebinding，TTL 设成 0 即可）。
///
/// 把校验放进 reqwest 自己用来建连的解析器里，**校验的就是将要连接的那组地址**，
/// 两次解析之间的时间窗不复存在。
///
/// IP 字面量不经过解析器，由 [`Client::ensure_public_target`] 单独检查。
struct PublicOnlyResolver;

/// 解析器拒绝内部地址时返回的错误，供上层从 reqwest 错误链里识别出来，
/// 转成语义明确的 [`Error::BlockedTarget`]（且不参与重试）。
#[derive(Debug)]
struct InternalTargetError {
    host: String,
}

impl std::fmt::Display for InternalTargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} resolves to an internal address", self.host)
    }
}

impl std::error::Error for InternalTargetError {}

impl reqwest::dns::Resolve for PublicOnlyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            // 端口填 0：reqwest 会用 URL 里的端口（或 scheme 默认端口）覆盖它
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            // 任一地址是内部地址就整体拒绝，而不是过滤掉它们只连公网的那几个：
            // 一个同时解析到公网与内网的域名本身就是可疑信号，且与字面量校验的
            // 「一票否决」语义保持一致
            if addrs.is_empty() || addrs.iter().any(|a| is_internal_ip(a.ip())) {
                return Err(Box::new(InternalTargetError { host }) as _);
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// 从 reqwest 的错误链里找出解析器抛出的 [`InternalTargetError`]。
fn blocked_by_resolver(err: &reqwest::Error) -> Option<String> {
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(err);
    while let Some(e) = source {
        if let Some(blocked) = e.downcast_ref::<InternalTargetError>() {
            return Some(blocked.host.clone());
        }
        source = e.source();
    }
    None
}

/// 重定向 `Location` 最大截断长度（字节），防止超长 Location 膨胀错误与日志。
const MAX_REDIRECT_LOCATION_BYTES: usize = 256;

/// `read_body_limited` 多块响应预分配上限（64 KiB），防止伪造超大 Content-Length 造成大分配。
const MAX_BODY_PREALLOC: usize = 64 * 1024;

/// 按 UTF-8 字符边界把重定向目标截断至不超过 [`MAX_REDIRECT_LOCATION_BYTES`] 字节。
fn truncate_location(location: &str) -> String {
    if location.len() <= MAX_REDIRECT_LOCATION_BYTES {
        return location.to_string();
    }
    let end = location.floor_char_boundary(MAX_REDIRECT_LOCATION_BYTES);
    location[..end].to_string()
}

/// `url` 是否已是 http(s) 绝对地址。
///
/// 必须匹配完整的 `http://` / `https://`，不能只看 `http` 前缀：`httpfoo/bar`
/// 这样的相对路径会被误判为绝对地址，于是 base_url 不再拼接、请求打到一个
/// 根本不存在的主机上。使用字节前缀比较（ASCII 大小写不敏感），避免在含多字节 UTF-8
/// 字符时按第 8 字节切片落入字符内部，同时消除临时 `String` 分配。
fn is_absolute_http_url(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"http://"))
        || bytes
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"https://"))
}

impl Client {
    /// 获取并发请求配额，超出 `max_processing` 时原子拒绝并返回 [`Error::ConcurrencyLimit`]。
    pub(crate) fn acquire_processing(&self) -> Result<ProcessingGuard> {
        if let Some(limit) = self.config.max_processing {
            self.processing
                .try_update(Ordering::AcqRel, Ordering::Acquire, |curr| {
                    (curr < limit).then_some(curr + 1)
                })
                .map_err(|_| Error::ConcurrencyLimit {
                    service: self.config.service.clone(),
                    limit,
                })?;
        } else {
            self.processing.fetch_add(1, Ordering::AcqRel);
        }
        Ok(ProcessingGuard {
            counter: Arc::clone(&self.processing),
        })
    }

    /// 已是 http(s) 绝对地址则直接使用，否则拼接 base_url。
    fn get_url(&self, url: &str) -> String {
        if is_absolute_http_url(url) {
            url.to_string()
        } else {
            // format! 单次分配；此前 base_url.to_string() + url 会分配两次（热路径每请求都走）
            format!("{}{url}", self.config.base_url)
        }
    }

    /// SSRF 校验（scheme 与 IP 字面量部分）：仅放行 http/https，且目标为内部地址则拒绝。
    ///
    /// 域名目标由挂在 reqwest 上的 [`PublicOnlyResolver`] 在**建连时**校验，
    /// 与实际连接使用同一次解析结果，不存在 DNS rebinding 时间窗。
    fn ensure_public_target(&self, url: &Url) -> Result<()> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(Error::BlockedTarget {
                service: self.config.service.clone(),
                host: format!("unsupported scheme: {}", url.scheme()),
            });
        }
        match url.host() {
            Some(Host::Ipv4(v4)) => {
                if is_internal_ipv4(v4) {
                    return Err(Error::BlockedTarget {
                        service: self.config.service.clone(),
                        host: v4.to_string(),
                    });
                }
            }
            Some(Host::Ipv6(v6)) => {
                if is_internal_ip(IpAddr::V6(v6)) {
                    return Err(Error::BlockedTarget {
                        service: self.config.service.clone(),
                        host: format!("[{v6}]"),
                    });
                }
            }
            Some(Host::Domain(domain)) => {
                // 兜底校验：防止调用方绕过或出现被当成域名的 IP 字面量
                let literal = domain.trim_start_matches('[').trim_end_matches(']');
                if let Ok(ip) = literal.parse::<IpAddr>()
                    && is_internal_ip(ip)
                {
                    return Err(Error::BlockedTarget {
                        service: self.config.service.clone(),
                        host: domain.to_string(),
                    });
                }
            }
            None => {
                return Err(Error::BlockedTarget {
                    service: self.config.service.clone(),
                    host: "<none>".to_string(),
                });
            }
        }
        // 域名不在这里解析：交给挂在 reqwest 上的 PublicOnlyResolver，
        // 让校验与建连用同一次解析结果，见其文档
        Ok(())
    }

    /// 读取响应体并在失败时通知熔断器记一次故障。
    pub(crate) async fn read_body_limited(
        &self,
        res: reqwest::Response,
        path: &str,
    ) -> Result<Bytes> {
        match self.read_body_limited_inner(res, path).await {
            Ok(bytes) => Ok(bytes),
            Err(err) => {
                if let Some(cb) = &self.config.circuit_breaker {
                    cb.record_failure();
                }
                Err(err)
            }
        }
    }

    /// 读取响应体，并在超过 `max_response_bytes` 时**中止**读取：
    /// 单块响应走零拷贝快路径，多块响应预分配封顶 [`MAX_BODY_PREALLOC`]（64 KiB）。
    async fn read_body_limited_inner(
        &self,
        mut res: reqwest::Response,
        path: &str,
    ) -> Result<Bytes> {
        let Some(limit) = self.config.max_response_bytes else {
            return res.bytes().await.with_context(|_| RequestSnafu {
                service: self.config.service.clone(),
                path: path.to_string(),
            });
        };

        // 对端如实声明 Content-Length 时直接拒绝，一个字节都不必读
        if let Some(declared) = res.content_length()
            && declared > limit as u64
        {
            return Err(Error::ResponseTooLarge {
                service: self.config.service.clone(),
                limit,
            });
        }

        let Some(first) = res.chunk().await.with_context(|_| RequestSnafu {
            service: self.config.service.clone(),
            path: path.to_string(),
        })?
        else {
            return Ok(Bytes::new());
        };
        if first.len() > limit {
            return Err(Error::ResponseTooLarge {
                service: self.config.service.clone(),
                limit,
            });
        }

        // 单块快路径：若第二块已 EOF，直接零拷贝返回 first，省去整份 BytesMut 分配与拷贝
        let Some(second) = res.chunk().await.with_context(|_| RequestSnafu {
            service: self.config.service.clone(),
            path: path.to_string(),
        })?
        else {
            return Ok(first);
        };
        let two_chunks_len = first.len().saturating_add(second.len());
        if two_chunks_len > limit {
            return Err(Error::ResponseTooLarge {
                service: self.config.service.clone(),
                limit,
            });
        }

        // 预分配取「声明长度」「上限」「64 KiB」的最小值，避免伪造超大 Content-Length 造成大分配
        let hint = res.content_length().map_or(8 * 1024, |len| {
            len.min(limit as u64).min(MAX_BODY_PREALLOC as u64) as usize
        });
        let mut buf = BytesMut::with_capacity(hint.max(two_chunks_len).min(limit));
        buf.extend_from_slice(&first);
        buf.extend_from_slice(&second);

        while let Some(chunk) = res.chunk().await.with_context(|_| RequestSnafu {
            service: self.config.service.clone(),
            path: path.to_string(),
        })? {
            if buf.len().saturating_add(chunk.len()) > limit {
                return Err(Error::ResponseTooLarge {
                    service: self.config.service.clone(),
                    limit,
                });
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf.freeze())
    }

    /// 共用请求准备：URL 解析、SSRF 检查、载荷/头部应用、OTel 注入、请求拦截器与熔断器预检。
    pub(crate) async fn prepare_request<Q, P>(
        &self,
        stats: &mut HttpStats,
        params: Params<'_, Q, P>,
    ) -> Result<RequestBuilder>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
    {
        let raw_url = self.get_url(params.url);
        let parsed_url = Url::parse(&raw_url).with_context(|_| UriSnafu {
            service: self.config.service.clone(),
        })?;
        stats.path = parsed_url.path().to_string();
        stats.method = params.method.to_string();

        // SSRF 防护：对开启的客户端，拒绝非 http(s) 或目标为内部地址的请求
        if self.config.deny_internal_targets {
            self.ensure_public_target(&parsed_url)?;
        }

        let mut req = self.client.request(params.method, parsed_url);
        if let Some(value) = params.timeout {
            req = req.timeout(value);
        }
        if let Some(value) = params.query {
            req = req.query(value);
        }
        if let Some(body) = params.body {
            req = match body {
                BodyPayload::Json(value) => req.json(value),
                BodyPayload::Form(value) => req.form(value),
                BodyPayload::Bytes { content_type, data } => {
                    if let Some(ct) = content_type {
                        req = req.header(CONTENT_TYPE, HeaderValue::from_static(ct));
                    }
                    req.body(Bytes::copy_from_slice(data))
                }
            };
        }
        // 调用方注入的单次请求头（如 webhook 签名 / 幂等键）；置于拦截器之前，
        // 仍可被后续拦截器的 request 钩子覆盖
        if let Some(headers) = params.headers {
            req = req.headers(headers.clone());
        }
        if !params.extra_headers.is_empty() {
            req = req.headers(params.extra_headers);
        }
        if let Some(token) = params.bearer_auth {
            req = req.bearer_auth(token);
        }
        // 注入当前 span 的 W3C trace 上下文（traceparent），让下游服务延续同一条调用链。
        // 无活跃 span 或未启用 OTel 时上下文为空，propagator 不写入任何头，零额外开销。
        let cx = tracing::Span::current().context();
        let mut trace_headers = HeaderMap::new();
        opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&cx, &mut HeaderInjector(&mut trace_headers));
        });
        if !trace_headers.is_empty() {
            req = req.headers(trace_headers);
        }
        // 依次调用各拦截器的 request 钩子（如注入鉴权头）
        if let Some(interceptors) = &self.config.interceptors {
            for interceptor in interceptors {
                req = interceptor.request(req).await?;
            }
        }
        // 熔断器打开 → 快速失败，不发起请求，保护持续故障的下游
        if let Some(cb) = &self.config.circuit_breaker
            && !cb.allow()
        {
            return Err(Error::CircuitOpen {
                service: self.config.service.clone(),
            });
        }
        Ok(req)
    }

    /// 共用发送与重试循环（供缓冲读取与流式读取复用）：
    /// 在重试退避 `sleep` 期间释放并发计数守卫，并在下次尝试前重新申请，避免长时间退避占用并发配额。
    pub(crate) async fn send_with_retry(
        &self,
        stats: &mut HttpStats,
        idempotent: bool,
        mut req: RequestBuilder,
        mut guard: ProcessingGuard,
    ) -> Result<(reqwest::Response, ProcessingGuard)> {
        let max_retries = self.config.max_retries;
        let mut attempt: u32 = 0;
        loop {
            // 预克隆以备下次重试；流式 body 不可克隆 → None（此请求不再重试）
            let retry_candidate = if attempt < max_retries {
                req.try_clone()
            } else {
                None
            };

            let process_done = Stopwatch::new();
            match req.send().await {
                Ok(res) => {
                    stats.processing = process_done.elapsed_ms();
                    if let Some(remote_addr) = res.remote_addr() {
                        stats.remote_addr = remote_addr.to_string();
                    }
                    // 在读 body 之前采集：证书信息挂在响应 extensions 上，
                    // `res.bytes()` 会消费掉 res
                    #[cfg(feature = "tls-info")]
                    {
                        stats.tls_cert = extract_tls_cert(&res);
                    }
                    let status = res.status().as_u16();
                    // SSRF 防护开启时不跟随重定向（build() 已设 Policy::none，这里把
                    // 收到的 3xx 显式转成错误）。Location 截断并带进错误信息。
                    if is_blocked_redirect(self.config.deny_internal_targets, status) {
                        stats.status = status;
                        let raw_location = res
                            .headers()
                            .get(LOCATION)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("<none>");
                        let location = truncate_location(raw_location);
                        return Err(Error::BlockedRedirect {
                            service: self.config.service.clone(),
                            location,
                        });
                    }
                    // 5xx / 429 且仍可重试 → 退避后重试（丢弃本次响应体，退避期间归还并发配额）
                    if is_retryable_status(status, idempotent)
                        && let Some(next) = retry_candidate
                    {
                        let retry_after = parse_retry_after(res.headers());
                        let delay = retry_after.unwrap_or_else(|| {
                            retry_backoff(self.config.retry_base_delay, attempt)
                        });
                        drop(res);
                        warn!(
                            target: LOG_TARGET,
                            service = self.config.service,
                            path = stats.path,
                            status,
                            attempt = attempt + 1,
                            delay_ms = delay.as_millis() as u64,
                            retry_after_honored = retry_after.is_some(),
                            "retry on server error",
                        );
                        drop(guard);
                        tokio::time::sleep(delay).await;
                        guard = self.acquire_processing()?;
                        req = next;
                        attempt += 1;
                        continue;
                    }
                    stats.status = status;
                    return Ok((res, guard));
                }
                Err(e) => {
                    // 解析器因目标是内网而拒绝：不重试、不计熔断失败。
                    if let Some(host) = blocked_by_resolver(&e) {
                        return Err(Error::BlockedTarget {
                            service: self.config.service.clone(),
                            host,
                        });
                    }
                    // 网络层错误：按方法幂等性决定是否重试
                    if should_retry_error(&e, idempotent)
                        && let Some(next) = retry_candidate
                    {
                        let delay = retry_backoff(self.config.retry_base_delay, attempt);
                        warn!(
                            target: LOG_TARGET,
                            service = self.config.service,
                            path = stats.path,
                            attempt = attempt + 1,
                            delay_ms = delay.as_millis() as u64,
                            error = %e,
                            "retry on network error",
                        );
                        drop(guard);
                        tokio::time::sleep(delay).await;
                        guard = self.acquire_processing()?;
                        req = next;
                        attempt += 1;
                        continue;
                    }
                    // 重试耗尽 / 不可重试 → 记一次熔断失败后返回网络错误
                    if let Some(cb) = &self.config.circuit_breaker {
                        cb.record_failure();
                    }
                    return Err(e).context(RequestSnafu {
                        service: self.config.service.clone(),
                        path: stats.path.clone(),
                    });
                }
            }
        }
    }

    /// 执行一次完整的缓冲式 HTTP 请求并收集响应，供 `execute` 与 `raw` 共享。
    async fn execute_buffered<Q, P>(
        &self,
        stats: &mut HttpStats,
        params: Params<'_, Q, P>,
        collect_headers: bool,
    ) -> Result<RawResponse>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
    {
        let guard = self.acquire_processing()?;
        let idempotent = is_idempotent(&params.method);
        let req = self.prepare_request(stats, params).await?;
        let (res, _guard) = self.send_with_retry(stats, idempotent, req, guard).await?;

        let status = res.status().as_u16();
        let headers = if collect_headers {
            res.headers().clone()
        } else {
            HeaderMap::new()
        };
        let transfer_done = Stopwatch::new();
        let body = self.read_body_limited(res, &stats.path).await?;
        stats.transfer = transfer_done.elapsed_ms();
        stats.content_length = body.len();
        stats.status = status;

        // 熔断计数：拿到完整响应后按状态码更新（5xx 计失败、其余视为恢复）
        if let Some(cb) = &self.config.circuit_breaker {
            if status >= 500 {
                cb.record_failure();
            } else {
                cb.record_success();
            }
        }

        Ok(RawResponse {
            status,
            headers,
            body,
        })
    }

    /// 执行 HTTP 请求并返回原始响应字节。
    /// 在公共发送逻辑之上追加拦截器 `fail` / `response` 钩子与 `≥400` 兜底校验。
    async fn raw<Q, P>(&self, stats: &mut HttpStats, params: Params<'_, Q, P>) -> Result<Bytes>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
    {
        let RawResponse {
            status, mut body, ..
        } = self.execute_buffered(stats, params, false).await?;

        if status >= 400 {
            if let Some(interceptors) = &self.config.interceptors {
                for interceptor in interceptors {
                    interceptor.fail(status, &body).await?;
                }
            }
            // 未注册失败转换拦截器（或自定义拦截器未处理 fail）时兜底转成 Error::Upstream，
            // 防止 ≥400 响应静默流入反序列化分支被当成 200 OK
            handle_fail(&self.config.service, status, &body)?;
        }
        if let Some(interceptors) = &self.config.interceptors {
            for interceptor in interceptors {
                body = interceptor.response(body).await?;
            }
        }
        Ok(body)
    }

    /// 执行请求并将响应体反序列化为指定类型，记录反序列化耗时。
    async fn do_request<Q, P, T>(
        &self,
        stats: &mut HttpStats,
        params: Params<'_, Q, P>,
    ) -> Result<T>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let full = self.raw(stats, params).await?;

        let serde_done = Stopwatch::new();
        let data = serde_json::from_slice(&full).with_context(|_| SerdeSnafu {
            service: self.config.service.clone(),
        })?;
        stats.serde = serde_done.elapsed_ms();
        Ok(data)
    }

    /// 内部通用请求入口：填充统计信息并在完成后触发 `on_done` 拦截器。
    async fn request<Q, P, T>(&self, params: Params<'_, Q, P>) -> Result<T>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let mut stats = HttpStats::default();
        let done = Stopwatch::new();
        let result = self.do_request(&mut stats, params).await;
        stats.total = done.elapsed_ms();
        run_on_done(&self.config, &stats, result.as_ref().err()).await;
        result
    }

    /// 发送请求并返回包含状态码、响应头与完整字节体的 [`RawResponse`]；
    /// 即使上游返回 ≥400 也会返回响应体，供需要自行解析上游错误结构的调用方使用。
    pub async fn execute<Q, P>(&self, params: Params<'_, Q, P>) -> Result<RawResponse>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
    {
        let mut stats = HttpStats::default();
        let done = Stopwatch::new();
        let result = self.execute_buffered(&mut stats, params, true).await;
        stats.total = done.elapsed_ms();
        run_on_done(&self.config, &stats, result.as_ref().err()).await;
        result
    }

    /// 发送请求并返回原始响应字节，不进行 JSON 反序列化。
    pub async fn request_raw<Q, P>(&self, params: Params<'_, Q, P>) -> Result<Bytes>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
    {
        let mut stats = HttpStats::default();
        let done = Stopwatch::new();
        let result = self.raw(&mut stats, params).await;
        stats.total = done.elapsed_ms();
        run_on_done(&self.config, &stats, result.as_ref().err()).await;
        result
    }

    /// 发送 GET 请求并将响应反序列化为 `T`。
    pub async fn get<T>(&self, url: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.request(Params::new(Method::GET, url)).await
    }

    /// 发送带查询参数的 GET 请求并将响应反序列化为 `T`。
    pub async fn get_with_query<P, T>(&self, url: &str, query: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params::new(Method::GET, url).with_query(query))
            .await
    }

    /// 发送带 JSON 请求体的 POST 请求并将响应反序列化为 `T`。
    pub async fn post<P, T>(&self, url: &str, json: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params::new(Method::POST, url).with_body(json))
            .await
    }

    /// 发送带 `application/x-www-form-urlencoded` 表单体的 POST 请求并将响应反序列化为 `T`。
    pub async fn post_form<P, T>(&self, url: &str, form: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params::new(Method::POST, url).with_form(form))
            .await
    }

    /// 发送带 JSON 请求体和查询参数的 POST 请求并将响应反序列化为 `T`。
    pub async fn post_with_query<P, Q, T>(&self, url: &str, json: &P, query: &Q) -> Result<T>
    where
        P: Serialize + ?Sized,
        Q: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(
            Params::new(Method::POST, url)
                .with_query(query)
                .with_body(json),
        )
        .await
    }

    /// 发送带 JSON 请求体的 PUT 请求并将响应反序列化为 `T`。
    pub async fn put<P, T>(&self, url: &str, json: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params::new(Method::PUT, url).with_body(json))
            .await
    }

    /// 发送带 JSON 请求体的 PATCH 请求并将响应反序列化为 `T`。
    pub async fn patch<P, T>(&self, url: &str, json: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params::new(Method::PATCH, url).with_body(json))
            .await
    }

    /// 发送 DELETE 请求并将响应反序列化为 `T`。
    pub async fn delete<T>(&self, url: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.request(Params::new(Method::DELETE, url)).await
    }

    /// 获取当前在途请求数。
    #[must_use]
    pub fn get_processing(&self) -> u32 {
        self.processing.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// SSRF：内网 / 特殊地址判为内部，公网地址判为外部。
    #[test]
    fn internal_ip_classification() {
        for ip in [
            "127.0.0.1",
            "::1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254", // 云元数据
            "100.64.0.1",      // CGNAT
            "0.0.0.0",
            "fc00::1",          // ULA
            "fe80::1",          // link-local
            "::ffff:127.0.0.1", // IPv4-mapped 回环
            "::ffff:10.0.0.1",  // IPv4-mapped 私网
            // 以下为补齐的网段（此前全部判为公网）
            "0.1.2.3",            // 0.0.0.0/8「本网络」，Linux 上可达本机
            "224.0.0.1",          // 组播
            "240.0.0.1",          // 保留
            "255.255.255.255",    // 广播
            "192.0.0.8",          // IETF 协议分配
            "198.18.0.1",         // 基准测试网段
            "64:ff9b::7f00:1",    // NAT64 夹带 127.0.0.1
            "64:ff9b::a9fe:a9fe", // NAT64 夹带 169.254.169.254（云元数据）
            "2002:7f00:1::",      // 6to4 夹带 127.0.0.1
            "2002:a00:1::",       // 6to4 夹带 10.0.0.1
            "ff02::1",            // IPv6 组播
            // 站点本地与新增 IANA 特殊/保留前缀
            "fec0::1",      // site-local
            "::127.0.0.1",  // IPv4-compatible（已废弃）
            "2001:db8::1",  // RFC 3849 文档前缀
            "3fff::1",      // RFC 9637 文档前缀
            "2001:2::1",    // RFC 5180 基准测试
            "2001::1",      // RFC 4380 Teredo 隧道
            "64:ff9b:1::1", // RFC 8215 NAT64 本地前缀
            "100::1",       // RFC 6666 丢弃前缀
        ] {
            assert!(
                is_internal_ip(ip.parse::<IpAddr>().unwrap()),
                "{ip} 应判为内部地址"
            );
        }
        for ip in [
            "1.1.1.1",
            "8.8.8.8",
            "9.9.9.9",
            "93.184.216.34",
            "2606:4700::1111",
            "2001:4860:4860::8888", // Google Public DNS（不应被 2001::/32 误伤）
            "3fff:1000::1",         // 超出 3fff::/20 范围的公网块
            // 夹带的是公网 v4 时应当放行，别误伤
            "64:ff9b::808:808", // NAT64 → 8.8.8.8
            "2002:808:808::",   // 6to4 → 8.8.8.8
        ] {
            assert!(
                !is_internal_ip(ip.parse::<IpAddr>().unwrap()),
                "{ip} 应判为公网地址"
            );
        }
    }

    /// `Retry-After` 只接受 delta-seconds，且必须有上界。
    #[test]
    fn retry_after_parses_seconds_and_caps_absurd_values() {
        let header = |v: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(RETRY_AFTER, HeaderValue::from_str(v).expect("合法头部值"));
            parse_retry_after(&headers)
        };

        assert_eq!(header("5"), Some(Duration::from_secs(5)));
        assert_eq!(header(" 30 "), Some(Duration::from_secs(30)));
        assert_eq!(header("0"), Some(Duration::ZERO));
        // 恰好等于上限仍接受
        assert_eq!(header("60"), Some(MAX_RETRY_AFTER));

        // 超出上限的值由对端指定，不可信——退回本地退避，别被挂住一整天
        assert_eq!(header("86400"), None);
        assert_eq!(header("61"), None);
        // HTTP-date 形式不支持，退回本地退避
        assert_eq!(header("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        assert_eq!(header("soon"), None);
        assert_eq!(header("-5"), None);
        // 没有这个头
        assert_eq!(parse_retry_after(&HeaderMap::new()), None);
    }

    /// 退避上界按 2 的幂增长，并在 2^6 处封顶。
    #[test]
    fn backoff_ceiling_grows_and_caps() {
        let base = Duration::from_millis(100);
        assert_eq!(retry_backoff_ceiling(base, 0), Duration::from_millis(100));
        assert_eq!(retry_backoff_ceiling(base, 1), Duration::from_millis(200));
        assert_eq!(retry_backoff_ceiling(base, 3), Duration::from_millis(800));
        // 指数封顶 2^6 = 64
        assert_eq!(
            retry_backoff_ceiling(base, 10),
            Duration::from_millis(100 * 64)
        );
    }

    /// 实际退避落在 `[ceiling/2, ceiling)`，且**多次调用不应相同**——
    /// 若有人把抖动去掉退回纯指数，这里会退化成单一取值而失败。
    #[test]
    fn backoff_is_jittered_within_half_of_ceiling() {
        let base = Duration::from_millis(100);
        for attempt in 0..4 {
            let ceiling = retry_backoff_ceiling(base, attempt);
            let samples: Vec<Duration> = (0..64).map(|_| retry_backoff(base, attempt)).collect();
            for d in &samples {
                assert!(
                    *d >= ceiling / 2 && *d < ceiling,
                    "attempt={attempt} 退避 {d:?} 越出 [{:?}, {ceiling:?})",
                    ceiling / 2
                );
            }
            let unique: HashSet<Duration> = samples.into_iter().collect();
            assert!(
                unique.len() > 1,
                "attempt={attempt} 的 64 次采样只得到一个值，抖动没生效"
            );
        }
    }

    /// 抖动不得破坏单调性：退避随重试次数增长。
    #[test]
    fn backoff_stays_monotonic_despite_jitter() {
        let base = Duration::from_millis(100);
        // 第 n 次的下界（ceiling/2）必须 ≥ 第 n-1 次的上界，故整体单调不重叠
        for attempt in 1..6 {
            let prev_max = retry_backoff_ceiling(base, attempt - 1);
            let curr_min = retry_backoff_ceiling(base, attempt) / 2;
            assert!(
                curr_min >= prev_max,
                "attempt={attempt} 的下界 {curr_min:?} 低于上一次的上界 {prev_max:?}"
            );
        }
    }

    /// 绝对地址判定必须匹配完整 scheme，不能只看 `http` 前缀。
    #[test]
    fn absolute_url_requires_full_scheme() {
        assert!(is_absolute_http_url("http://example.com/a"));
        assert!(is_absolute_http_url("https://example.com/a"));
        // scheme 大小写不敏感
        assert!(is_absolute_http_url("HTTPS://example.com"));
        assert!(is_absolute_http_url("HtTp://example.com"));

        // 回归守卫：这些是相对路径，必须拼 base_url
        assert!(!is_absolute_http_url("httpfoo/bar"));
        assert!(!is_absolute_http_url("https-proxy/status"));
        assert!(!is_absolute_http_url("/http/health"));
        assert!(!is_absolute_http_url("http"));
        assert!(!is_absolute_http_url(""));
        // 非 http scheme 不算（交给 base_url 拼接后由解析器或 SSRF 策略拒绝）
        assert!(!is_absolute_http_url("ftp://example.com"));
        // 含多字节 UTF-8 时第 8 字节可能落在字符中间，不能 panic 或误判为相对路径
        assert!(is_absolute_http_url("http://中文.example/a"));
        assert!(is_absolute_http_url("https://中"));
    }

    /// 幂等性分类：GET/PUT 幂等，POST/PATCH 非幂等。
    #[test]
    fn idempotency_classification() {
        assert!(is_idempotent(&Method::GET));
        assert!(is_idempotent(&Method::PUT));
        assert!(!is_idempotent(&Method::POST));
        assert!(!is_idempotent(&Method::PATCH));
    }

    /// 仅幂等方法按 429 / 5xx 重试。
    #[test]
    fn retryable_status_only_for_idempotent() {
        assert!(is_retryable_status(503, true));
        assert!(is_retryable_status(429, true));
        assert!(!is_retryable_status(503, false));
        assert!(!is_retryable_status(404, true));
    }

    /// 证书剩余有效期：未过期为正、已过期为负。
    #[test]
    fn cert_expiry_is_signed_distance() {
        let cert = TlsCertInfo {
            not_before: 1_000,
            not_after: 2_000,
        };
        assert_eq!(cert.expires_in_secs(1_500), 500);
        assert_eq!(cert.expires_in_secs(2_000), 0);
        assert_eq!(cert.expires_in_secs(2_600), -600, "已过期必须是负数");
    }

    /// 真实自签证书的 DER：有效期须与 openssl 生成时指定的固定日期一致。
    /// fixture 用固定 notBefore/notAfter 生成，故本例不随时间漂移。
    #[cfg(feature = "tls-info")]
    #[test]
    fn parse_cert_validity_reads_real_der() {
        const DER: &[u8] = include_bytes!("../tests/fixtures/self_signed.der");
        let cert = parse_cert_validity(DER).expect("自签证书应能解析");
        // 2024-01-02T03:04:05Z / 2034-01-02T03:04:05Z
        assert_eq!(cert.not_before, 1_704_164_645);
        assert_eq!(cert.not_after, 2_019_783_845);
    }

    /// 非法 DER 不得 panic，只返回 None——证书信息是观测数据，坏了也不能影响请求。
    #[cfg(feature = "tls-info")]
    #[test]
    fn parse_cert_validity_rejects_garbage() {
        assert_eq!(parse_cert_validity(b"not a certificate"), None);
        assert_eq!(parse_cert_validity(&[]), None);
    }

    /// SSRF 防护开启时 3xx 一律拦截；未开启则不干预（由 reqwest 自动跟随）。
    #[test]
    fn redirect_blocked_only_when_ssrf_guard_on() {
        for status in [300, 301, 302, 303, 307, 308, 399] {
            assert!(
                is_blocked_redirect(true, status),
                "{status} 在开启 SSRF 防护时必须拦截，否则可被 302 到内网绕过"
            );
            assert!(!is_blocked_redirect(false, status));
        }
        // 边界：299 / 400 不是重定向
        for status in [200, 299, 400, 404, 500] {
            assert!(!is_blocked_redirect(true, status));
        }
    }

    /// 熔断器：达阈值打开、冷却内拒绝、冷却后恢复、成功清零。
    #[test]
    fn circuit_breaker_opens_and_recovers() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(50));
        assert!(!cb.is_open.load(Ordering::Acquire));
        assert!(cb.allow());
        cb.record_failure(); // 1 次，未达阈值
        assert!(!cb.is_open.load(Ordering::Acquire));
        assert!(cb.allow());
        cb.record_failure(); // 达阈值 → 打开
        assert!(cb.is_open.load(Ordering::Acquire));
        assert!(!cb.allow()); // 冷却窗口内快速失败
        std::thread::sleep(Duration::from_millis(60));
        assert!(cb.allow()); // 冷却结束恢复（半开单探针）
        cb.record_success(); // 成功清零计数并关闭快速路径标志
        assert!(!cb.is_open.load(Ordering::Acquire));
        assert!(cb.allow());
    }

    /// Client / ClientBuilder / Params 的 Debug 输出不得泄漏默认请求头或 Bearer 凭证。
    #[test]
    fn debug_redacts_secret_headers_and_tokens() {
        let secret = "sk-super-secret-api-key-987654";
        let builder = ClientBuilder::new("llm").with_header(
            http::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {secret}")).unwrap(),
        );
        let builder_dbg = format!("{builder:?}");
        assert!(!builder_dbg.contains(secret), "{builder_dbg}");
        assert!(builder_dbg.contains("header_count: 1"), "{builder_dbg}");

        let client = builder.build().unwrap();
        let client_dbg = format!("{client:?}");
        assert!(!client_dbg.contains(secret), "{client_dbg}");

        let params = Params::new(Method::POST, "/v1/chat").with_bearer_auth(secret);
        let params_dbg = format!("{params:?}");
        assert!(!params_dbg.contains(secret), "{params_dbg}");
        assert!(params_dbg.contains("[REDACTED]"), "{params_dbg}");
    }

    /// 开启 SSRF 防护时，dns_overrides 不允许空列表或内网 IP；未开启时仍禁止空列表。
    #[test]
    fn dns_overrides_validation_rejects_empty_and_internal_under_ssrf() {
        let mut empty = HashMap::new();
        empty.insert("api.example.com".to_string(), Vec::new());
        let err = ClientBuilder::new("test")
            .with_dns_overrides(empty)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Common { .. }));

        let mut internal = HashMap::new();
        internal.insert(
            "api.example.com".to_string(),
            vec!["169.254.169.254:80".parse::<SocketAddr>().unwrap()],
        );
        let err = ClientBuilder::new("ssrf")
            .with_deny_internal_targets()
            .with_dns_overrides(internal)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::BlockedTarget { .. }));
    }

    /// 超长 UTF-8 重定向 Location 截断后不超过 256 字节且保持合法 UTF-8。
    #[test]
    fn truncate_location_respects_char_boundary() {
        let repeated = "https://example.com/".to_string() + &"中".repeat(120);
        assert!(repeated.len() > MAX_REDIRECT_LOCATION_BYTES);
        let truncated = truncate_location(&repeated);
        assert!(truncated.len() <= MAX_REDIRECT_LOCATION_BYTES);
        assert!(repeated.starts_with(&truncated));
    }

    /// 并发达上限时返回 ConcurrencyLimit（503），且计数器不会虚增或残留。
    #[tokio::test]
    async fn concurrency_limit_returns_503_and_keeps_counter_clean() {
        let client = ClientBuilder::new("limit-test")
            .with_max_processing(0)
            .build()
            .unwrap();
        let err = client
            .get::<serde_json::Value>("http://127.0.0.1:1/test")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ConcurrencyLimit { limit: 0, .. }));
        assert_eq!(client.get_processing(), 0);
        let base = BaseError::from(err);
        assert_eq!(base.status(), 503);
    }

    /// `Client::execute` 透传 ≥400 响应供上层解析，而未挂拦截器的 `get()` 兜底转成 Upstream 错误。
    #[tokio::test]
    async fn execute_returns_error_status_while_get_falls_back_to_handle_fail() {
        use axum::routing::{get, post};

        let app =
            axum::Router::new()
                .route(
                    "/fail",
                    get(|| async { (http::StatusCode::BAD_REQUEST, r#"{"message":"bad"}"#) }),
                )
                .route(
                    "/form",
                    post(
                        |axum::Form(fields): axum::Form<HashMap<String, String>>| async move {
                            axum::Json(serde_json::json!({ "k": fields.get("k") }))
                        },
                    ),
                )
                .route(
                    "/raw-json",
                    post(|body: Bytes| async move {
                        axum::Json(serde_json::json!({ "len": body.len() }))
                    }),
                );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = ClientBuilder::new("svc")
            .with_base_url(format!("http://{addr}"))
            .build()
            .unwrap();

        let raw_resp = client
            .execute(Params::new(Method::GET, "/fail"))
            .await
            .unwrap();
        assert_eq!(raw_resp.status, 400);
        assert!(!raw_resp.is_success());

        let err = client.get::<serde_json::Value>("/fail").await.unwrap_err();
        assert!(matches!(err, Error::Upstream { status: 400, .. }));

        let form_resp: serde_json::Value = client.post_form("/form", &[("k", "v")]).await.unwrap();
        assert_eq!(form_resp["k"], "v");

        let json_bytes_resp: serde_json::Value = client
            .request_raw(Params::new(Method::POST, "/raw-json").with_json_bytes(b"{\"a\":1}"))
            .await
            .map(|b| serde_json::from_slice(&b).unwrap())
            .unwrap();
        assert_eq!(json_bytes_resp["len"], 7);
    }
}
