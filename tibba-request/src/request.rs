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
use axum::http::Method;
use axum::http::header::HeaderMap;
use axum::http::uri::Uri;
use bytes::Bytes;
use reqwest::Client as ReqwestClient;
use reqwest::RequestBuilder;
use scopeguard::defer;
use serde::Serialize;
use serde::de::DeserializeOwned;
use snafu::ResultExt;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tibba_util::{Stopwatch, json_get};
use tracing::{info, warn};

type Result<T> = std::result::Result<T, Error>;

/// 装箱的异步 Future，用于 trait object 场景下的异步方法返回类型。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// crate 版本号，注入 User-Agent。
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 空查询参数占位符，避免调用方每次传 `None::<&[(&str, &str)]>`。
const EMPTY_QUERY: Option<&[(&str, &str)]> = None;
/// 空请求体占位符。
const EMPTY_BODY: Option<&[(&str, &str)]> = None;

/// HTTP 请求参数，泛型 `Q` 为查询参数类型，`P` 为请求体类型，均须实现 `Serialize`。
#[derive(Clone, Debug, Default)]
pub struct Params<'a, Q, P>
where
    Q: Serialize + ?Sized,
    P: Serialize + ?Sized,
{
    /// HTTP 方法
    pub method: Method,
    /// 单次请求超时，覆盖客户端默认值；`None` 则沿用客户端配置。
    pub timeout: Option<Duration>,
    /// URL 查询参数
    pub query: Option<&'a Q>,
    /// JSON 请求体
    pub body: Option<&'a P>,
    /// 请求 URL（绝对地址或相对于 base_url 的路径）
    pub url: &'a str,
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
    /// TLS 版本
    pub tls_version: String,
    /// TLS 证书有效期起始时间
    pub tls_not_before: String,
    /// TLS 证书有效期截止时间
    pub tls_not_after: String,
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
    fn on_done(&self, stats: &HttpStats, err: Option<&Error>) -> BoxFuture<'_, Result<()>> {
        let error = err.map(ToString::to_string);
        let service = self.service.clone();
        let method = stats.method.clone();
        let path = stats.path.clone();
        let status = stats.status;
        let remote_addr = stats.remote_addr.clone();
        let content_length = stats.content_length;
        let processing = stats.processing;
        let transfer = stats.transfer;
        let serde = stats.serde;
        let total = stats.total;
        Box::pin(async move {
            info!(
                target: LOG_TARGET,
                service,
                method,
                path,
                status,
                remote_addr,
                content_length,
                processing,
                transfer,
                serde,
                total,
                error,
            );
            Ok(())
        })
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

/// 指数退避：`base * 2^attempt`，指数封顶 2^6（64 倍）防止过长等待。
fn retry_backoff(base: Duration, attempt: u32) -> Duration {
    let factor = 1u32 << attempt.min(6);
    base.saturating_mul(factor)
}

/// 简单熔断器：连续失败达 `threshold` 即打开，`cooldown` 内对请求快速失败；冷却结束后
/// 自动恢复（下一次失败会再次打开）。非严格 half-open，MVP 足够。状态用原子 + 极短临界区
/// 维护，**不跨 await 持锁**。
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

    /// 是否放行本次请求。处于打开窗口内 → `false`；冷却结束 → 复位并放行。
    fn allow(&self) -> bool {
        let mut guard = self.open_until.lock().unwrap_or_else(|e| e.into_inner());
        match *guard {
            Some(until) if Instant::now() < until => false,
            // 冷却结束，恢复放行（失败再开）
            Some(_) => {
                *guard = None;
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
}

/// HTTP 客户端构建器，通过链式调用配置后调用 `.build()` 生成 `Client`。
pub struct ClientBuilder {
    config: ClientConfig,
}

impl ClientBuilder {
    /// 以服务名创建构建器，其余选项均使用默认值。
    pub fn new(service: &str) -> Self {
        Self {
            config: ClientConfig {
                service: service.to_string(),
                base_url: String::new(),
                read_timeout: None,
                timeout: None,
                connect_timeout: None,
                pool_idle_timeout: None,
                pool_max_idle_per_host: 0,
                headers: None,
                interceptors: None,
                max_processing: None,
                dns_overrides: None,
                max_retries: 0,
                retry_base_delay: Duration::from_millis(100),
                circuit_breaker: None,
            },
        }
    }

    /// 设置基础 URL，相对路径请求将拼接在此 URL 之后。
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.config.base_url = base_url.into();
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
    #[must_use]
    pub fn with_retry(mut self, max_retries: u32, base_delay: Duration) -> Self {
        self.config.max_retries = max_retries;
        self.config.retry_base_delay = base_delay;
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
        // 启用 TLS 信息采集，供拦截器读取证书有效期等元数据
        builder = builder.tls_info(true);

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

impl Client {
    /// 若 `url` 以 "http" 开头则直接使用，否则拼接 base_url。
    fn get_url(&self, url: &str) -> String {
        if url.starts_with("http") {
            url.to_string()
        } else {
            self.config.base_url.to_string() + url
        }
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
        let uri = url.parse::<Uri>().context(UriSnafu {
            service: self.config.service.clone(),
        })?;
        stats.path = uri.path().to_string();
        stats.method = params.method.to_string();

        // 幂等性决定重试策略（在 match 消费前先算好）
        let idempotent = is_idempotent(&params.method);
        let mut req = match params.method {
            Method::POST => self.client.post(url),
            Method::PUT => self.client.put(url),
            Method::PATCH => self.client.patch(url),
            Method::DELETE => self.client.delete(url),
            _ => self.client.get(url),
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
                    let status = res.status().as_u16();
                    let transfer_done = Stopwatch::new();
                    let body = res.bytes().await.context(RequestSnafu {
                        service: self.config.service.clone(),
                        path: stats.path.clone(),
                    })?;
                    stats.transfer = transfer_done.elapsed_ms();

                    // 5xx / 429 且仍可重试 → 退避后重试
                    if is_retryable_status(status, idempotent)
                        && let Some(next) = retry_candidate
                    {
                        let delay = retry_backoff(self.config.retry_base_delay, attempt);
                        warn!(
                            target: LOG_TARGET,
                            service = self.config.service,
                            path = stats.path,
                            status,
                            attempt = attempt + 1,
                            delay_ms = delay.as_millis() as u64,
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
        let data = serde_json::from_slice(&full).context(SerdeSnafu {
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
        self.request(Params {
            timeout: None,
            method: Method::GET,
            url,
            query: EMPTY_QUERY,
            body: EMPTY_BODY,
        })
        .await
    }

    /// 发送带查询参数的 GET 请求并将响应反序列化为 `T`。
    pub async fn get_with_query<P, T>(&self, url: &str, query: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params {
            timeout: None,
            method: Method::GET,
            url,
            query: Some(query),
            body: EMPTY_BODY,
        })
        .await
    }

    /// 发送带 JSON 请求体的 POST 请求并将响应反序列化为 `T`。
    pub async fn post<P, T>(&self, url: &str, json: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params {
            timeout: None,
            method: Method::POST,
            url,
            query: EMPTY_QUERY,
            body: Some(json),
        })
        .await
    }

    /// 发送带 JSON 请求体和查询参数的 POST 请求并将响应反序列化为 `T`。
    pub async fn post_with_query<P, Q, T>(&self, url: &str, json: &P, query: &Q) -> Result<T>
    where
        P: Serialize + ?Sized,
        Q: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params {
            timeout: None,
            method: Method::POST,
            url,
            query: Some(query),
            body: Some(json),
        })
        .await
    }

    /// 发送带 JSON 请求体的 PUT 请求并将响应反序列化为 `T`。
    pub async fn put<P, T>(&self, url: &str, json: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params {
            timeout: None,
            method: Method::PUT,
            url,
            query: EMPTY_QUERY,
            body: Some(json),
        })
        .await
    }

    /// 发送带 JSON 请求体的 PATCH 请求并将响应反序列化为 `T`。
    pub async fn patch<P, T>(&self, url: &str, json: &P) -> Result<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Params {
            timeout: None,
            method: Method::PATCH,
            url,
            query: EMPTY_QUERY,
            body: Some(json),
        })
        .await
    }

    /// 发送 DELETE 请求并将响应反序列化为 `T`。
    pub async fn delete<T>(&self, url: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.request(Params {
            timeout: None,
            method: Method::DELETE,
            url,
            query: EMPTY_QUERY,
            body: EMPTY_BODY,
        })
        .await
    }

    /// 获取当前在途请求数。
    pub fn get_processing(&self) -> u32 {
        self.processing.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 退避按 2 的幂增长，并在 2^6 处封顶。
    #[test]
    fn backoff_grows_and_caps() {
        let base = Duration::from_millis(100);
        assert_eq!(retry_backoff(base, 0), Duration::from_millis(100));
        assert_eq!(retry_backoff(base, 1), Duration::from_millis(200));
        assert_eq!(retry_backoff(base, 3), Duration::from_millis(800));
        // 指数封顶 2^6 = 64
        assert_eq!(retry_backoff(base, 10), Duration::from_millis(100 * 64));
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
