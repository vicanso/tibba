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

//! Lua 脚本执行，以及建立在其上的两个原子原语：带属主的分布式锁、滑动窗口限流。
//!
//! ## 为什么这些非要用 Lua
//! 两者都需要「读-判断-写」在服务端一次完成。拆成多条命令，中间那一瞬就是漏洞：
//!
//! - **解锁**：`GET` 确认是自己的锁、再 `DEL`——两步之间锁可能已过期并被另一个
//!   实例重新持有，于是你删掉的是**别人**的锁。这也正是
//!   [`RedisCache::lock`] 此前只能靠 TTL 过期、刻意不提供 `unlock` 的原因。
//! - **滑动窗口**：`ZREMRANGEBYSCORE` 清理过期 + `ZCARD` 计数 + `ZADD` 记录，
//!   三步之间的并发请求会读到同一个计数，配额被击穿。
//!
//! ## EVALSHA 与回退
//! [`RedisCache::eval`] 走 `redis::Script`，它先试 `EVALSHA`（只发 40 字节摘要），
//! 服务端报 `NOSCRIPT` 时自动改发完整 `EVAL` 并让服务端缓存下来。因此脚本正文
//! 只在每个 Redis 实例的首次调用（或 `SCRIPT FLUSH` / 重启之后）传输一次。
//!
//! ## 集群注意
//! 脚本涉及的所有 key 必须落在同一个 slot。本模块的两个原语都只用**一个** key，
//! 天然满足；自定义脚本用多 key 时需自行用 hash tag（`{tag}`）约束。

use super::{Error, RedisCache, RedisSnafu};
use redis::{FromRedisValue, Script};
use snafu::ResultExt;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tibba_util::nanoid;

type Result<T> = std::result::Result<T, Error>;

/// 属主校验后再删除：CAS 解锁。
///
/// `GET` 与 `DEL` 之间不能有任何间隙——有间隙就可能删掉别人刚拿到的锁。
static UNLOCK_SCRIPT: LazyLock<Script> = LazyLock::new(|| {
    Script::new(
        r"
if redis.call('GET', KEYS[1]) == ARGV[1] then
    return redis.call('DEL', KEYS[1])
end
return 0
",
    )
});

/// 滑动窗口限流：清理窗口外记录 → 计数 → 未超限则记录本次。
///
/// 返回 `{allowed, count, retry_after_ms}`。
///
/// 用有序集合而非计数器：固定窗口（`INCR` + `EXPIRE`）在窗口交界处会放行接近
/// **两倍**配额——窗口末尾打满一轮，跨过边界立刻又能打满一轮。滑动窗口以每条
/// 记录自己的时间戳为准，不存在这个边界效应。
static SLIDING_WINDOW_SCRIPT: LazyLock<Script> = LazyLock::new(|| {
    Script::new(
        r"
local now = tonumber(ARGV[1])
local window = tonumber(ARGV[2])
local limit = tonumber(ARGV[3])

-- 清掉窗口之外的记录，集合因此不会无限增长
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', now - window)
local count = redis.call('ZCARD', KEYS[1])

if count >= limit then
    -- 最老的一条何时滑出窗口，即调用方最早何时可以重试
    local oldest = redis.call('ZRANGE', KEYS[1], 0, 0, 'WITHSCORES')
    local retry = 0
    if oldest[2] then
        retry = math.ceil(tonumber(oldest[2]) + window - now)
        if retry < 0 then retry = 0 end
    end
    return {0, count, retry}
end

redis.call('ZADD', KEYS[1], now, ARGV[4])
-- 整个集合的存活时间就是一个窗口：最后一条记录滑出后它自然消失
redis.call('PEXPIRE', KEYS[1], window)
return {1, count + 1, 0}
",
    )
});

/// 当前 Unix 毫秒时间戳。
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

/// 分布式锁的属主令牌。
///
/// 由 [`RedisCache::lock_with_token`] 生成，解锁时必须原样交回
/// [`RedisCache::unlock`]——这正是「只删自己的锁」得以成立的依据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockToken(String);

impl LockToken {
    /// 令牌的字符串形式，供需要跨进程传递的场景使用。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 由字符串还原令牌（例如从任务上下文里取回）。
    #[must_use]
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// 一次限流判定的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitStatus {
    /// 本次是否放行
    pub allowed: bool,
    /// 窗口内已记录的请求数（放行时包含本次）
    pub count: i64,
    /// 窗口内允许的上限
    pub limit: i64,
    /// 被拒时，最早可重试的等待时长；放行时为 `None`。
    /// 可直接用于 `Retry-After` 响应头。
    pub retry_after: Option<Duration>,
}

impl RateLimitStatus {
    /// 窗口内还剩多少配额，被拒时为 0。
    #[must_use]
    pub fn remaining(&self) -> i64 {
        (self.limit - self.count).max(0)
    }
}

impl RedisCache {
    /// 执行 Lua 脚本。`keys` 会自动加上本实例的键前缀，`args` 原样传给 `ARGV`。
    ///
    /// `args` 用 `&[&str]` 不是限制而是事实：Redis 的 `ARGV` 在 Lua 侧**始终**是
    /// 字符串，数值需要在脚本里 `tonumber()`。
    ///
    /// 优先 `EVALSHA`，服务端未缓存时自动回退 `EVAL`，见模块文档。
    pub async fn eval<T>(&self, script: &Script, keys: &[&str], args: &[&str]) -> Result<T>
    where
        T: FromRedisValue,
    {
        let mut conn = self.conn().await?;
        let mut invocation = script.prepare_invoke();
        for key in keys {
            invocation.key(self.get_key(key).as_ref());
        }
        for arg in args {
            invocation.arg(*arg);
        }
        invocation
            .invoke_async(&mut conn)
            .await
            .context(RedisSnafu { category: "eval" })
    }

    /// 获取一把**带属主**的分布式锁，成功返回解锁所需的令牌。
    ///
    /// 与 [`RedisCache::lock`] 的分工：
    ///
    /// | | `lock` | `lock_with_token` |
    /// |---|--------|-------------------|
    /// | 语义 | 同一时间窗内全集群只跑一次 | 互斥临界区 |
    /// | 释放 | 只靠 TTL 过期 | [`Self::unlock`] 主动释放，或 TTL 兜底 |
    /// | 典型场景 | 定时任务去重 | 订单扣减、账户结算 |
    ///
    /// 定时任务去重**不要**用本方法：任务跑完就解锁，会让同一触发窗口内的其它
    /// 实例立刻抢到锁重跑一遍，正好破坏「只跑一次」的目的。
    ///
    /// 临界区仍须幂等：执行超过 `ttl` 时锁会自然过期并可能被他人持有。
    pub async fn lock_with_token(
        &self,
        key: &str,
        ttl: Option<Duration>,
    ) -> Result<Option<LockToken>> {
        // 令牌必须不可预测且不重复：它是「这把锁是我的」的唯一凭据
        let token = nanoid(24);
        let acquired: bool = redis::cmd("SET")
            .arg(self.get_key(key))
            .arg(&token)
            .arg("NX")
            .arg("PX")
            .arg(self.get_ttl_ms(ttl))
            .query_async(&mut self.conn().await?)
            .await
            .context(RedisSnafu {
                category: "lock_with_token",
            })?;
        Ok(acquired.then_some(LockToken(token)))
    }

    /// 释放 [`Self::lock_with_token`] 取得的锁。
    ///
    /// 返回 `true` 表示确实由本次调用释放；`false` 表示锁已不属于该令牌——
    /// 要么早已过期，要么已被他人重新持有。**这种情况不是错误**，但它说明临界区
    /// 的执行时间超过了 `ttl`，值得调用方记一条日志。
    ///
    /// 校验与删除在同一个 Lua 脚本里完成。分成 `GET` + `DEL` 两步的写法存在一个
    /// 致命窗口：两步之间锁可能过期并被另一个实例拿走，于是 `DEL` 删掉的是别人
    /// 的锁，互斥当场失效。
    pub async fn unlock(&self, key: &str, token: &LockToken) -> Result<bool> {
        let deleted: i64 = self.eval(&UNLOCK_SCRIPT, &[key], &[token.as_str()]).await?;
        Ok(deleted > 0)
    }

    /// 滑动窗口限流：判定本次请求是否放行，并记录它。
    ///
    /// 跨实例共享配额（状态在 Redis），且没有固定窗口的边界效应——后者在窗口
    /// 交界处会放行接近**两倍**配额（窗口末尾打满一轮，跨过边界立刻再打满一轮），
    /// 这正是 `incr` 固定窗口方案的固有缺陷。
    ///
    /// 代价是每个 key 要存 `limit` 条时间戳记录（而非一个计数器），并且 key 的
    /// 存活时间就是一个窗口。`limit` 极大时应改用固定窗口或令牌桶。
    ///
    /// `limit` ≤ 0 时一律拒绝，不访问 Redis。
    pub async fn rate_limit_sliding(
        &self,
        key: &str,
        limit: i64,
        window: Duration,
    ) -> Result<RateLimitStatus> {
        if limit <= 0 {
            return Ok(RateLimitStatus {
                allowed: false,
                count: 0,
                limit,
                retry_after: Some(window),
            });
        }
        let now = now_millis();
        let window_ms = u64::try_from(window.as_millis()).unwrap_or(u64::MAX).max(1);
        // 同一毫秒内的并发请求必须是不同成员，否则 ZADD 只会覆盖分数、少记一次
        let member = format!("{now}-{}", nanoid(10));

        let values: Vec<i64> = self
            .eval(
                &SLIDING_WINDOW_SCRIPT,
                &[key],
                &[
                    &now.to_string(),
                    &window_ms.to_string(),
                    &limit.to_string(),
                    &member,
                ],
            )
            .await?;

        Ok(parse_rate_limit(&values, limit, window))
    }
}

/// 把脚本返回的 `{allowed, count, retry_after_ms}` 解析成 [`RateLimitStatus`]。
///
/// 抽成自由函数以便单测覆盖边界，无需真实 Redis。
fn parse_rate_limit(values: &[i64], limit: i64, window: Duration) -> RateLimitStatus {
    let allowed = values.first().copied().unwrap_or(0) == 1;
    let count = values.get(1).copied().unwrap_or(0);
    let retry_ms = values.get(2).copied().unwrap_or(0);
    RateLimitStatus {
        allowed,
        count,
        limit,
        // 被拒时至少给 1ms，避免调用方拿到 0 后立刻重试打成忙等
        retry_after: (!allowed).then(|| {
            if retry_ms > 0 {
                Duration::from_millis(retry_ms.unsigned_abs())
            } else {
                window.min(Duration::from_millis(1))
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn lock_token_round_trips_through_string() {
        let token = LockToken::from_string("abc123");
        assert_eq!(token.as_str(), "abc123");
        assert_eq!(token, LockToken::from_string("abc123".to_string()));
        assert_ne!(token, LockToken::from_string("other"));
    }

    #[test]
    fn allowed_result_has_no_retry_after() {
        let status = parse_rate_limit(&[1, 3, 0], 10, Duration::from_secs(60));
        assert!(status.allowed);
        assert_eq!(status.count, 3);
        assert_eq!(status.remaining(), 7);
        assert_eq!(status.retry_after, None);
    }

    #[test]
    fn rejected_result_carries_retry_after() {
        let status = parse_rate_limit(&[0, 10, 1500], 10, Duration::from_secs(60));
        assert!(!status.allowed);
        assert_eq!(status.count, 10);
        assert_eq!(status.remaining(), 0, "超限时剩余配额应为 0，不得为负");
        assert_eq!(status.retry_after, Some(Duration::from_millis(1500)));
    }

    /// 被拒但脚本给不出重试时刻时，也不能返回 0——调用方会据此立刻重试，打成忙等。
    #[test]
    fn rejected_without_retry_hint_still_waits() {
        let status = parse_rate_limit(&[0, 10, 0], 10, Duration::from_secs(60));
        assert!(!status.allowed);
        assert_eq!(status.retry_after, Some(Duration::from_millis(1)));
    }

    /// 返回值残缺（脚本被人改坏 / 版本不匹配）时按「拒绝」处理，fail-closed。
    #[test]
    fn malformed_script_reply_fails_closed() {
        for values in [vec![], vec![0], vec![7, 7]] {
            let status = parse_rate_limit(&values, 10, Duration::from_secs(60));
            assert!(!status.allowed, "残缺返回值 {values:?} 不得被当成放行");
            assert!(status.retry_after.is_some());
        }
    }

    /// `remaining` 永不为负。
    #[test]
    fn remaining_is_clamped_at_zero() {
        let status = parse_rate_limit(&[0, 99, 10], 10, Duration::from_secs(60));
        assert_eq!(status.remaining(), 0);
    }

    /// 两段脚本都要能被 `redis::Script` 接受并算出稳定的 SHA。
    #[test]
    fn scripts_have_stable_hashes() {
        let unlock = UNLOCK_SCRIPT.get_hash().to_string();
        let sliding = SLIDING_WINDOW_SCRIPT.get_hash().to_string();

        assert_eq!(unlock.len(), 40, "SHA-1 十六进制应为 40 字符");
        assert_eq!(sliding.len(), 40);
        assert_ne!(unlock, sliding);
        // LazyLock 每次取到的是同一份，SHA 不应变化
        assert_eq!(unlock, UNLOCK_SCRIPT.get_hash());
    }

    #[test]
    fn now_millis_is_a_plausible_unix_timestamp() {
        // 2020-01-01 之后、且在 i64 毫秒范围内
        assert!(now_millis() > 1_577_836_800_000);
    }
}
