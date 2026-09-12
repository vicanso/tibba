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

//! 通用速率限制中间件，基于 [`governor`] 的进程内令牌桶。
//!
//! 与现有 `user_tracker`（账号粒度并发计数）正交：
//! - `user_tracker`：登录态/账号维度的并发上限，存 Redis
//! - 本模块：**IP 维度**的速率限制，**进程内**，**无 Redis 依赖**
//!
//! ## ⚠️ 多实例部署的取舍
//! governor 是**进程内**令牌桶：N 个实例 = N 倍配额。需要严格跨实例共享上限时
//! 应改用 Redis 滑动窗口；本中间件适合的场景：
//! - 单实例 / 少实例部署的粗略限流
//! - 已有上游 LB / WAF 做集群级限流时的内部兜底
//! - 高并发场景下接受偏差换零网络往返开销
//!
//! ## 用法
//!
//! ```ignore
//! use std::num::NonZeroU32;
//! use axum::middleware::from_fn_with_state;
//! use tibba_middleware::{ip_rate_limit, IpRateLimitState};
//!
//! // 每个 IP 每分钟 60 次；State 可复用到多个路由（共享同一桶）
//! let limit_state = IpRateLimitState::per_minute(NonZeroU32::new(60).unwrap());
//!
//! Router::new()
//!     .route("/api/sensitive", post(handler))
//!     .layer(from_fn_with_state(limit_state, ip_rate_limit))
//! ```

use crate::{ClientIp, Error, LOG_TARGET};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::response::IntoResponse;
use axum::middleware::Next;
use axum::response::Response;
use governor::clock::DefaultClock;
use governor::state::keyed::DashMapStateStore;
use governor::{Quota, RateLimiter};
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tracing::debug;

type Result<T, E = BaseError> = std::result::Result<T, E>;

/// 由 IP 地址作 key 的 governor 限制器。
type IpKeyedLimiter = RateLimiter<IpAddr, DashMapStateStore<IpAddr>, DefaultClock>;

/// 共享给中间件 closure 的状态：限制器 + 配额展示用文本。
#[derive(Clone)]
pub struct IpRateLimitState {
    limiter: Arc<IpKeyedLimiter>,
    quota_text: Arc<String>,
}

impl IpRateLimitState {
    /// 构造一个"每分钟 `per_minute` 次"的 IP 限流器。
    pub fn per_minute(per_minute: NonZeroU32) -> Self {
        let limiter = Arc::new(RateLimiter::keyed(Quota::per_minute(per_minute)));
        Self {
            limiter,
            quota_text: Arc::new(format!("{per_minute}/min")),
        }
    }

    /// 构造一个"每秒 `per_second` 次"的 IP 限流器（突发场景）。
    pub fn per_second(per_second: NonZeroU32) -> Self {
        let limiter = Arc::new(RateLimiter::keyed(Quota::per_second(per_second)));
        Self {
            limiter,
            quota_text: Arc::new(format!("{per_second}/sec")),
        }
    }
}

/// 中间件 fn：按 client IP 校验配额。超限返回 HTTP 429。
pub async fn ip_rate_limit(
    State(state): State<IpRateLimitState>,
    ClientIp(ip): ClientIp,
    req: Request,
    next: Next,
) -> Result<Response> {
    match state.limiter.check_key(&ip) {
        Ok(_) => Ok(next.run(req).await),
        Err(_not_until) => {
            debug!(target: LOG_TARGET, ip = %ip, quota = %state.quota_text, "ip rate limit hit");
            Err(Error::RateLimited {
                quota: state.quota_text.to_string(),
            }
            .into())
        }
    }
}

/// Redis 限流的窗口算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RedisWindow {
    /// 滑动窗口（默认）：有序集合逐条记录请求时刻，按真实时间窗判定。
    ///
    /// 没有边界效应，代价是每个 key 存最多 `max` 条时间戳。
    #[default]
    Sliding,
    /// 固定窗口：单计数器 + TTL，最省内存。
    ///
    /// **代价**：窗口交界处最多可放行接近**两倍**配额——窗口末尾打满一轮，
    /// 跨过边界立刻又能打满一轮。`max` 很大、内存敏感、且能接受这个偏差时才选它。
    Fixed,
}

/// 基于 Redis 的 IP 限流状态：跨实例共享配额。
///
/// 相比 [`IpRateLimitState`]（governor 内存计数，每实例独立配额），多副本部署下此实现
/// 全局一致——所有实例读写同一份 Redis 状态，配额不会因扩容而放大。
///
/// 默认走**滑动窗口**（[`RedisWindow::Sliding`]）。此前只有固定窗口，
/// 在窗口交界处会放行接近两倍配额——对登录、发信这类正是要防爆破的端点，
/// 这个偏差不能忽略。需要旧行为用 [`Self::with_window`]。
#[derive(Clone)]
pub struct RedisIpRateLimit {
    cache: &'static RedisCache,
    /// 命名空间，区分不同端点的配额桶（如 "login" / "email"）。
    label: &'static str,
    /// 窗口内允许的最大请求数。
    max: i64,
    /// 计数窗口长度。
    window: Duration,
    /// 窗口算法，默认滑动窗口。
    strategy: RedisWindow,
}

impl RedisIpRateLimit {
    /// `label` 用于隔离不同端点的计数键；`max` 为窗口内上限；`window` 为窗口长度。
    #[must_use]
    pub fn new(
        cache: &'static RedisCache,
        label: &'static str,
        max: i64,
        window: Duration,
    ) -> Self {
        Self {
            cache,
            label,
            max,
            window,
            strategy: RedisWindow::default(),
        }
    }

    /// 选择窗口算法，支持链式调用。默认 [`RedisWindow::Sliding`]。
    #[must_use]
    pub fn with_window(mut self, strategy: RedisWindow) -> Self {
        self.strategy = strategy;
        self
    }
}

/// 中间件：按 client IP 在 Redis 窗口内计数，超限返回 429。跨实例共享配额。
///
/// 被拒时带上 `Retry-After` 响应头（秒）：客户端据此知道该等多久，而不是立刻重试
/// 把已经过载的端点继续打满。滑动窗口能给出精确到毫秒的等待时长（最老一条记录
/// 何时滑出窗口），固定窗口只能给出整个窗口长度作为保守上界。
///
/// Redis 不可用时错误上抛（fail-closed，宁可拒绝也不放过高频请求）；本应用
/// 会话本就强依赖 Redis，故不额外引入可用性耦合。
pub async fn redis_ip_rate_limit(
    State(state): State<RedisIpRateLimit>,
    ClientIp(ip): ClientIp,
    req: Request,
    next: Next,
) -> Result<Response> {
    let key = format!("rate:{}:{ip}", state.label);

    let (allowed, retry_after) = match state.strategy {
        RedisWindow::Sliding => {
            let status = state
                .cache
                .rate_limit_sliding(&key, state.max, state.window)
                .await?;
            (status.allowed, status.retry_after)
        }
        RedisWindow::Fixed => {
            // incr 原子自增并在首次设窗口 TTL（见 RedisCache::incr）
            let count = state.cache.incr(&key, 1, Some(state.window)).await?;
            // 固定窗口拿不到「本 IP 的窗口何时结束」，只能给窗口长度这个上界
            (count <= state.max, Some(state.window))
        }
    };

    if allowed {
        return Ok(next.run(req).await);
    }

    debug!(target: LOG_TARGET, ip = %ip, label = state.label, "redis ip rate limit hit");
    let err: BaseError = Error::RateLimited {
        quota: format!("{}/{}s", state.max, state.window.as_secs()),
    }
    .into();
    // 直接构造响应而非 `Err(..)?`：要在错误响应上补 Retry-After 头。
    // 完整 Error 仍随 into_response 进入 extensions，tracker / 日志不受影响。
    let mut res = err.into_response();
    if let Some(value) = retry_after_header(retry_after) {
        res.headers_mut().insert(header::RETRY_AFTER, value);
    }
    Ok(res)
}

/// 把等待时长渲染成 `Retry-After` 头值（秒，向上取整，至少 1）。
///
/// 规范只接受整秒或 HTTP-date；亚秒等待向上取整到 1 秒——报 `0` 等于邀请客户端
/// 立刻重试，比不给这个头更糟。
fn retry_after_header(retry_after: Option<Duration>) -> Option<HeaderValue> {
    let wait = retry_after?;
    let secs = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
    HeaderValue::from_str(&secs.max(1).to_string()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn header_value(d: Option<Duration>) -> Option<String> {
        retry_after_header(d).map(|v| v.to_str().unwrap_or_default().to_string())
    }

    /// `Retry-After` 只接受整秒，亚秒等待必须向上取整——
    /// 报 0 等于邀请客户端立刻重试，比不给这个头更糟。
    #[test]
    fn retry_after_rounds_up_to_whole_seconds() {
        assert_eq!(header_value(Some(Duration::from_millis(1))).as_deref(), Some("1"));
        assert_eq!(header_value(Some(Duration::from_millis(999))).as_deref(), Some("1"));
        assert_eq!(header_value(Some(Duration::from_secs(1))).as_deref(), Some("1"));
        assert_eq!(header_value(Some(Duration::from_millis(1001))).as_deref(), Some("2"));
        assert_eq!(header_value(Some(Duration::from_secs(60))).as_deref(), Some("60"));
        // 零等待同样至少给 1 秒
        assert_eq!(header_value(Some(Duration::ZERO)).as_deref(), Some("1"));
    }

    #[test]
    fn no_retry_hint_yields_no_header() {
        assert_eq!(header_value(None), None);
    }

    /// 默认必须是滑动窗口：固定窗口在交界处会放行接近两倍配额，
    /// 而这个中间件正是挂在登录 / 发信这类要防爆破的端点上。
    #[test]
    fn default_strategy_is_sliding() {
        assert_eq!(RedisWindow::default(), RedisWindow::Sliding);
    }
}
