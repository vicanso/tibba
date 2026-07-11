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

/// 基于 Redis 的 IP 限流状态：跨实例共享计数（固定窗口）。
///
/// 相比 [`IpRateLimitState`]（governor 内存计数，每实例独立配额），多副本部署下此实现
/// 全局一致——所有实例累加同一个 Redis 计数器，配额不会因扩容而放大。
#[derive(Clone)]
pub struct RedisIpRateLimit {
    cache: &'static RedisCache,
    /// 命名空间，区分不同端点的配额桶（如 "login" / "email"）。
    label: &'static str,
    /// 窗口内允许的最大请求数。
    max: i64,
    /// 计数窗口长度。
    window: Duration,
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
        }
    }
}

/// 中间件：按 client IP 在 Redis 固定窗口内计数，超限返回 429。跨实例共享配额。
///
/// Redis 不可用时 `incr` 返回错误并上抛（fail-closed，宁可拒绝也不放过高频请求）；本应用
/// 会话本就强依赖 Redis，故不额外引入可用性耦合。
pub async fn redis_ip_rate_limit(
    State(state): State<RedisIpRateLimit>,
    ClientIp(ip): ClientIp,
    req: Request,
    next: Next,
) -> Result<Response> {
    let key = format!("rate:{}:{ip}", state.label);
    // incr 原子自增并在首次设窗口 TTL（见 RedisCache::incr）
    let count = state.cache.incr(&key, 1, Some(state.window)).await?;
    if count > state.max {
        debug!(target: LOG_TARGET, ip = %ip, label = state.label, "redis ip rate limit hit");
        return Err(Error::RateLimited {
            quota: format!("{}/{}s", state.max, state.window.as_secs()),
        }
        .into());
    }
    Ok(next.run(req).await)
}
