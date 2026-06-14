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

//! 登录暴力破解防护 —— 基于 Redis 固定窗口计数的失败锁定。
//!
//! 在凭证校验「之前」用 [`ensure_not_locked`] 拦截：账号或来源 IP 在窗口内失败
//! 次数超阈值，直接拒绝（连密码都不再比对，杜绝爆破）。凭证错误时
//! [`record_failure`] 累加计数；成功登录时 [`clear_failures`] 清账号计数。
//!
//! ## 双维度
//! - **账号维度**：阈值较低（默认 5 次 / 15 分钟），针对单账号定向爆破
//! - **IP 维度**：阈值较高（默认 30 次 / 15 分钟），针对撞库（同 IP 扫多账号）；
//!   阈值放宽以减少 NAT / 办公出口共享 IP 的误伤
//!
//! 计数键用 [`RedisCache::incr`]（内部 `EXPIRE NX`）实现**固定窗口**：首次失败
//! 起算，窗口内不滑动，到期由 Redis 自动解锁，无需额外清理任务。

use crate::Error;
use std::time::Duration;
use tibba_cache::RedisCache;
use tracing::warn;

/// 本模块日志 target，可用 `RUST_LOG=tibba:router_user=info` 过滤。
const LOG_TARGET: &str = "tibba:router_user";

/// 账号维度失败计数键前缀。
const ACCOUNT_PREFIX: &str = "login_fail:acct:";
/// IP 维度失败计数键前缀。
const IP_PREFIX: &str = "login_fail:ip:";
/// 账号维度阈值：窗口内失败达到该值即锁定。
const ACCOUNT_MAX_FAILURES: i64 = 5;
/// IP 维度阈值：放宽以容忍共享出口 IP。
const IP_MAX_FAILURES: i64 = 30;
/// 计数窗口（固定窗口，到期自动解锁）。
const WINDOW: Duration = Duration::from_secs(15 * 60);

type Result<T> = std::result::Result<T, tibba_error::Error>;

fn account_key(account: &str) -> String {
    format!("{ACCOUNT_PREFIX}{account}")
}

fn ip_key(ip: &str) -> String {
    format!("{IP_PREFIX}{ip}")
}

/// 登录前置闸门：账号或 IP 在窗口内失败过多则返回 429（[`Error::TooManyAttempts`]），
/// 调用方应据此中止后续凭证校验。Redis 读失败时向上传播（应用本就强依赖 Redis）。
pub(crate) async fn ensure_not_locked(cache: &RedisCache, account: &str, ip: &str) -> Result<()> {
    let acct: Option<i64> = cache.get(&account_key(account)).await?;
    if acct.unwrap_or(0) >= ACCOUNT_MAX_FAILURES {
        return Err(Error::TooManyAttempts.into());
    }
    let ip_count: Option<i64> = cache.get(&ip_key(ip)).await?;
    if ip_count.unwrap_or(0) >= IP_MAX_FAILURES {
        return Err(Error::TooManyAttempts.into());
    }
    Ok(())
}

/// 记录一次登录失败（账号 + IP 双维度累加）。
///
/// 尽力而为：Redis 故障仅告警，不把一次正常的 401 升级成 500。
pub(crate) async fn record_failure(cache: &RedisCache, account: &str, ip: &str) {
    if let Err(e) = cache.incr(&account_key(account), 1, Some(WINDOW)).await {
        warn!(target: LOG_TARGET, error = %e, "record account login failure failed");
    }
    if let Err(e) = cache.incr(&ip_key(ip), 1, Some(WINDOW)).await {
        warn!(target: LOG_TARGET, error = %e, "record ip login failure failed");
    }
}

/// 登录成功后清除账号失败计数。
///
/// 仅清账号维度：IP 计数刻意保留，避免攻击者借一次成功登录给整段 IP 解锁。
pub(crate) async fn clear_failures(cache: &RedisCache, account: &str) {
    if let Err(e) = cache.del(&account_key(account)).await {
        warn!(target: LOG_TARGET, error = %e, "clear account login failures failed");
    }
}
