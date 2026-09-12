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
use axum::http::header::RETRY_AFTER;
use axum::http::Method;
use axum::http::header::{HeaderMap, HeaderName, HeaderValue, LOCATION};
use axum::http::uri::Uri;
use bytes::{Bytes, BytesMut};
use reqwest::Client as ReqwestClient;
use reqwest::RequestBuilder;
use scopeguard::defer;
use serde::Serialize;
use serde::de::DeserializeOwned;
use snafu::ResultExt;
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tibba_util::{Stopwatch, json_get, timestamp};
use tracing::{info, warn};
use tracing_opentelemetry::OpenTelemetrySpanExt;

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
#[derive(Clone, Debug, Default)]
pub struct Params<'a, Q = NoParams, P = NoParams>
where
    Q: Serialize + ?Sized,
    P: Serialize + ?Sized,
{
    /// HTTP 方法
    method: Method,
    /// 单次请求超时，覆盖客户端默认值；`None` 则沿用客户端配置。
    timeout: Option<Duration>,
    /// URL 查询参数
    query: Option<&'a Q>,
    /// JSON 请求体
    body: Option<&'a P>,
    /// 请求 URL（绝对地址或相对于 base_url 的路径）
    url: &'a str,
    /// 单次请求附加头（如 webhook 签名、幂等键、追踪头）；在拦截器之前应用，
    /// 仍可被后续拦截器覆盖。`None` 则不附加。
    headers: Option<&'a HeaderMap>,
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

    /// 设置本次请求附加的请求头，支持链式调用。
    #[must_use]
    pub fn with_headers(mut self, headers: &'a HeaderMap) -> Self {
        self.headers = Some(headers);
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
            body: Some(body),
            url: self.url,
            headers: self.headers,
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
async fn run_on_done(config: &ClientConfig, stats: &HttpStats, err: Option<&Error>) {
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

/// 从响应体中提取 `message` 字段，状态码 ≥400 时构造业务错误。
pub fn handle_fail(service: &str, status: u16, data: &Bytes) -> Result<()> {
    if status >= 400 {
        let mut message = json_get(data, "message");
        if message.is_empty() {
            message = "unknown error".to_string();
        }
        return Err(Error::Common {
            service: service.to_string(),
            message,
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
fn is_idempotent(method: &Method) -> bool {
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
struct CircuitBreaker {
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
    fn record_success(&self) {
        self.failures.store(0, Ordering::Relaxed);
        let mut guard = self.open_until.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    /// 记一次失败：达阈值则打开熔断（设置冷却截止时刻）。
    fn record_failure(&self) {
        let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if failures >= self.threshold {
            let until = Instant::now() + self.cooldown;
            let mut guard = self.open_until.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(until);
        }
    }
}

/// HTTP 客户端内部配置，由 `ClientBuilder` 填充后转移给 `Client`。
struct ClientConfig {
    /// 服务名称，用于日志和错误标识
    service: String,
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
    /// 最大并发在途请求数，超出时返回 "too many requests" 错误
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
    circuit_breaker: Option<CircuitBreaker>,
    /// 开启后拒绝目标解析到内部地址（私网 / 回环 / 链路本地 / 云元数据）的请求，防 SSRF。
    /// 对投递到用户 / 运维可控 URL 的客户端（webhook、探测）应开启。
    deny_internal_targets: bool,
    /// 响应体字节上限；`None` = 不限制（仅在调用方显式关闭时出现）
    max_response_bytes: Option<usize>,
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
        // SSRF 防护开启时必须关掉自动跟随：reqwest 默认最多跟 10 跳，而
        // ensure_public_target 只在首次发送前校验一次，跟随即等于绕过。
        // 见 ClientBuilder::with_deny_internal_targets 的说明。
        if self.config.deny_internal_targets {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }

        let client = builder.build().context(BuildSnafu {
            service: self.config.service.clone(),
        })?;
        Ok(Client {
            client,
            config: self.config,
            processing: AtomicU32::new(0),
        })
    }
}

/// HTTP 客户端，封装 reqwest `Client`，提供带拦截器链和并发限制的请求方法。
pub struct Client {
    /// 底层 reqwest 客户端
    client: ReqwestClient,
    /// 客户端配置（服务名、超时、拦截器等）
    config: ClientConfig,
    /// 当前在途请求数，用于并发限制
    processing: AtomicU32,
}

/// 判断 IP 是否为内部 / 特殊地址，SSRF 场景下应拒绝：
/// 回环、私网、链路本地（含 169.254.169.254 云元数据）、CGNAT、文档段、未指定 / 广播，
/// 以及 IPv6 的 ULA / link-local / IPv4-mapped 内部地址。
fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                // 共享地址空间 100.64.0.0/10（运营商级 NAT）
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            // IPv4-mapped（::ffff:a.b.c.d）按内嵌 v4 判断，防绕过
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_internal_ip(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00 // ULA fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
        }
    }
}

/// `url` 是否已是 http(s) 绝对地址。
///
/// 必须匹配完整的 `http://` / `https://`，不能只看 `http` 前缀：`httpfoo/bar`
/// 这样的相对路径会被误判为绝对地址，于是 base_url 不再拼接、请求打到一个
/// 根本不存在的主机上。scheme 按 RFC 3986 大小写不敏感。
fn is_absolute_http_url(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or_default().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

impl Client {
    /// 已是 http(s) 绝对地址则直接使用，否则拼接 base_url。
    fn get_url(&self, url: &str) -> String {
        if is_absolute_http_url(url) {
            url.to_string()
        } else {
            // format! 单次分配；此前 base_url.to_string() + url 会分配两次（热路径每请求都走）
            format!("{}{url}", self.config.base_url)
        }
    }

    /// SSRF 校验：确保目标 host 解析出的所有 IP 都是公网地址，否则拒绝。
    ///
    /// host 为 IP 字面量时直接校验；为域名时经 DNS 解析后逐个校验。注意：reqwest 建连时会
    /// **再次**解析域名，理论上存在 DNS-rebinding 时间窗；此实现足以挡住直连内网 IP、云元数据
    /// 端点与静态解析到内网的域名（绝大多数真实 SSRF），rebinding 属更高级攻击，后续可用
    /// 连接前 IP 固定（resolve_to_addrs）进一步收敛。
    async fn ensure_public_target(&self, uri: &Uri) -> Result<()> {
        let host = uri.host().ok_or_else(|| Error::BlockedTarget {
            service: self.config.service.clone(),
            host: "<none>".to_string(),
        })?;
        let port = uri.port_u16().unwrap_or(443);

        let addrs: Vec<IpAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![ip]
        } else {
            tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| Error::Common {
                    service: self.config.service.clone(),
                    message: format!("dns resolve failed for {host}: {e}"),
                })?
                .map(|sa| sa.ip())
                .collect()
        };

        // 解析结果为空，或任一 IP 为内部地址，都拒绝
        if addrs.is_empty() || addrs.iter().any(|ip| is_internal_ip(*ip)) {
            return Err(Error::BlockedTarget {
                service: self.config.service.clone(),
                host: host.to_string(),
            });
        }
        Ok(())
    }

    /// 读取响应体，并在超过 `max_response_bytes` 时**中止**读取。
    ///
    /// 关键是「中止」而非「读完再判断」：后者该占的内存早就占了，限制形同虚设。
    /// 这里逐块累加并在越界的那一块就返回，`res` 随即被 drop、连接关闭，对端
    /// 再往下发多少都与我们无关。
    ///
    /// 未配置上限时走 `Response::bytes()` 的快路径，不额外分配。
    async fn read_body_limited(&self, mut res: reqwest::Response, path: &str) -> Result<Bytes> {
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

        // 预分配取「声明长度」与「上限」的较小值：Content-Length 可能是伪造的
        // 天文数字，照它预分配本身就是一次 OOM
        let capacity = res
            .content_length()
            .map_or(8 * 1024, |len| len.min(limit as u64) as usize);
        let mut buf = BytesMut::with_capacity(capacity);

        while let Some(chunk) = res.chunk().await.with_context(|_| RequestSnafu {
            service: self.config.service.clone(),
            path: path.to_string(),
        })? {
            if buf.len() + chunk.len() > limit {
                return Err(Error::ResponseTooLarge {
                    service: self.config.service.clone(),
                    limit,
                });
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf.freeze())
    }

    /// 执行 HTTP 请求并返回原始响应字节。
    /// 负责并发计数、拦截器链调用（request / fail / response）及统计采集。
    async fn raw<Q, P>(&self, stats: &mut HttpStats, params: Params<'_, Q, P>) -> Result<Bytes>
    where
        Q: Serialize + ?Sized,
        P: Serialize + ?Sized,
    {
        let processing = self.processing.fetch_add(1, Ordering::Relaxed) + 1;
        defer! {
            self.processing.fetch_sub(1, Ordering::Relaxed);
        };
        // 超出并发限制时立即拒绝
        if let Some(max_processing) = self.config.max_processing
            && processing > max_processing
        {
            return Err(Error::Common {
                service: self.config.service.clone(),
                message: "too many requests".to_string(),
            });
        }

        let url = self.get_url(params.url);
        // 用 with_context 而非 context：后者会**立即**求值上下文表达式，于是
        // `service.clone()` 在每个成功请求上也照跑一遍。闭包版只在出错时分配。
        let uri = url.parse::<Uri>().with_context(|_| UriSnafu {
            service: self.config.service.clone(),
        })?;
        stats.path = uri.path().to_string();
        stats.method = params.method.to_string();

        // SSRF 防护：对开启的客户端，拒绝目标解析到内部地址的请求
        if self.config.deny_internal_targets {
            self.ensure_public_target(&uri).await?;
        }

        // 幂等性决定重试策略（在 match 消费前先算好）
        let idempotent = is_idempotent(&params.method);
        let mut req = match params.method {
            Method::POST => self.client.post(url),
            Method::PUT => self.client.put(url),
            Method::PATCH => self.client.patch(url),
            Method::DELETE => self.client.delete(url),
            Method::GET => self.client.get(url),
            // HEAD / OPTIONS 等其余方法按原始 method 构造，避免静默降级为 GET
            method => self.client.request(method, url),
        };
        if let Some(value) = params.timeout {
            req = req.timeout(value);
        }
        if let Some(value) = params.query {
            req = req.query(value);
        }
        if let Some(value) = params.body {
            req = req.json(value);
        }
        // 调用方注入的单次请求头（如 webhook 签名 / 幂等键）；置于拦截器之前，
        // 仍可被后续拦截器的 request 钩子覆盖
        if let Some(headers) = params.headers {
            req = req.headers(headers.clone());
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

        // 重试循环：失败按指数退避重排，达上限后返回最后一次结果 / 错误
        let max_retries = self.config.max_retries;
        let mut attempt: u32 = 0;
        let (status, mut full) = loop {
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
                    // 收到的 3xx 显式转成错误）。在读 body 之前返回：3xx 的响应体没有
                    // 价值，交给下游反序列化只会退化成语义不明的 JSON 解析失败，把真正
                    // 的原因盖掉。Location 带进错误信息，便于运维定位是哪个目标在跳转。
                    if is_blocked_redirect(self.config.deny_internal_targets, status) {
                        stats.status = status;
                        let location = res
                            .headers()
                            .get(LOCATION)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("<none>")
                            .to_string();
                        return Err(Error::BlockedRedirect {
                            service: self.config.service.clone(),
                            location,
                        });
                    }
                    // 读 body 之前先留下重试要用的 Retry-After：`res` 马上会被消费掉
                    let retry_after = parse_retry_after(res.headers());
                    let transfer_done = Stopwatch::new();
                    let body = self.read_body_limited(res, &stats.path).await?;
                    stats.transfer = transfer_done.elapsed_ms();

                    // 5xx / 429 且仍可重试 → 退避后重试
                    if is_retryable_status(status, idempotent)
                        && let Some(next) = retry_candidate
                    {
                        // 对端明确给了 Retry-After 就照办：它比我们的本地猜测更准，
                        // 也是 429 场景下唯一能真正避免继续挨打的做法。
                        // 超出 MAX_RETRY_AFTER 的值不可信，退回本地退避。
                        let delay = retry_after
                            .unwrap_or_else(|| retry_backoff(self.config.retry_base_delay, attempt));
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
                        tokio::time::sleep(delay).await;
                        req = next;
                        attempt += 1;
                        continue;
                    }
                    break (status, body);
                }
                Err(e) => {
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
                        tokio::time::sleep(delay).await;
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
        };
        stats.content_length = full.len();
        stats.status = status;

        // 熔断计数：拿到响应后按状态码更新（5xx 计失败、其余视为恢复）
        if let Some(cb) = &self.config.circuit_breaker {
            if status >= 500 {
                cb.record_failure();
            } else {
                cb.record_success();
            }
        }

        if let Some(interceptors) = &self.config.interceptors {
            // 状态码 ≥400 时触发各拦截器的 fail 钩子
            if status >= 400 {
                for interceptor in interceptors {
                    interceptor.fail(status, &full).await?;
                }
            }
            // 依次调用各拦截器的 response 钩子（如解包外层结构）
            for interceptor in interceptors {
                full = interceptor.response(full).await?;
            }
        }
        Ok(full)
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
        // 非 http scheme 不算（交给 base_url 拼接后由 reqwest 报错更明确）
        assert!(!is_absolute_http_url("ftp://example.com"));
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
        assert!(cb.allow());
        cb.record_failure(); // 1 次，未达阈值
        assert!(cb.allow());
        cb.record_failure(); // 达阈值 → 打开
        assert!(!cb.allow()); // 冷却窗口内快速失败
        std::thread::sleep(Duration::from_millis(60));
        assert!(cb.allow()); // 冷却结束恢复
        cb.record_success(); // 成功清零计数
        assert!(cb.allow());
    }
}
