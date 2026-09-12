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

//! 缓存旁路（cache-aside）+ 进程内请求合并（singleflight）。
//!
//! ## 解决什么
//! 「读缓存 → 没有就回源 → 写回缓存」是最常见的缓存用法，但手写它有两个坑：
//!
//! 1. **缓存击穿**：热点 key 失效的那一瞬，所有并发请求同时发现未命中，一起回源。
//!    数据库看到的是一根尖峰，而缓存本来就是为了削掉它。
//! 2. **忘了做**：`tibba-cache` 自己的热点路径约定表里写着「API Key 校验 →
//!    短 TTL 缓存 → 避免每次请求查 DB」，但因为没有现成的 API，实际实现里
//!    每个请求仍在打两次数据库。没有顺手可用的东西，约定就只是约定。
//!
//! [`RedisCache::get_or_set`] 把两件事一起解决。
//!
//! ## singleflight 的边界：**进程内**
//! 合并只在单个进程内生效。N 个实例同时未命中，数据库仍会看到 N 次回源——但
//! 那是 N，不是 N × 并发数，而后者才是真正压垮数据库的量级。要做到全集群只回源
//! 一次需要分布式锁（[`RedisCache::lock_with_token`]），代价是每次未命中都多一次
//! Redis 往返，且锁持有者崩溃时其余请求要等锁过期。对绝大多数场景不划算。

use super::RedisCache;
use dashmap::DashMap;
use serde::{Serialize, de::DeserializeOwned};
use std::future::Future;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tibba_error::Error as BaseError;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// 按完整缓存键（含前缀）分组的回源互斥锁。
///
/// 全局共享：键已含前缀，天然唯一，不同 [`RedisCache`] 实例之间也不会串。
static FLIGHTS: LazyLock<DashMap<String, Arc<Mutex<()>>>> = LazyLock::new(DashMap::new);

/// 持有某个 key 的回源权，`Drop` 时释放并清理注册表。
///
/// 必须靠 RAII 而不是在函数末尾手动清理：回源闭包可能返回 `Err`、也可能被取消
/// （客户端断开时整个请求 future 被 drop），任何一条路径漏掉清理，这个 key 就
/// 永远卡在「有人正在回源」的状态上。
struct Flight {
    key: String,
    _guard: OwnedMutexGuard<()>,
}

impl Drop for Flight {
    fn drop(&mut self) {
        // 仅当没有别的任务还拿着这个 Arc 时才移除。
        // 无条件移除会有一个竞态：A 移除后 B 才拿到旧 Arc，于是 C 从表里取到
        // 新建的 Arc——两把不同的锁守着同一个 key，合并当场失效。
        //
        // strong_count == 2：一份在表里，一份是本 guard 持有的。
        FLIGHTS.remove_if(&self.key, |_, lock| Arc::strong_count(lock) <= 2);
    }
}

/// 取得 `key` 的回源权，已有任务在回源时在此等待。
async fn begin_flight(key: String) -> Flight {
    let lock = FLIGHTS.entry(key.clone()).or_default().clone();
    let guard = lock.lock_owned().await;
    Flight { key, _guard: guard }
}

impl RedisCache {
    /// 读缓存；未命中则调用 `loader` 回源、写回缓存并返回。
    ///
    /// 同一进程内对同一 key 的并发未命中会被合并成**一次** `loader` 调用，其余
    /// 调用方等待并直接复用结果。
    ///
    /// # 负缓存
    /// `T` 取 `Option<U>` 即可把「查不到」也缓存下来。这通常正是想要的——否则
    /// 一串无效 ID（或无效令牌）会每次都打到数据库，成了一个免费的放大器。
    ///
    /// ```ignore
    /// // 无效令牌的查询结果（None）同样被缓存，挡住无效令牌洪水
    /// let auth: Option<ApiKeyAuth> = cache
    ///     .get_or_set("apikey:<hash>", Some(Duration::from_secs(30)), || async {
    ///         Ok(ApiKeyModel::new().find_active_by_hash(pool, hash).await?)
    ///     })
    ///     .await?;
    /// ```
    ///
    /// # 错误语义
    /// 返回 [`tibba_error::Error`] 而非本 crate 的 `Error`——`loader` 是调用方的
    /// 代码，它的错误类型不可能是缓存模块的。缓存自身的错误照常经 `From` 转换。
    ///
    /// `loader` 失败时**不写缓存**，错误直接上抛；下一次调用会重新尝试回源。
    ///
    /// # 一致性
    /// 数据变更后需要调用方显式 [`RedisCache::del`] 失效，否则最多陈旧一个 `ttl`。
    /// 对「可以短暂陈旧」的读多写少数据（鉴权信息、配置、字典表）这是合适的取舍；
    /// 要求强一致的数据不要用缓存。
    pub async fn get_or_set<T, F, Fut>(
        &self,
        key: &str,
        ttl: Option<Duration>,
        loader: F,
    ) -> std::result::Result<T, BaseError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = std::result::Result<T, BaseError>>,
    {
        // 第一次读：绝大多数请求在这里就返回了，不碰锁
        if let Some(value) = self.get_struct::<T>(key).await? {
            return Ok(value);
        }

        let _flight = begin_flight(self.get_key(key).into_owned()).await;

        // 拿到回源权后**再读一次**：等锁期间很可能已经有人填好了。
        // 少了这一步，合并就退化成「排队逐个回源」，一次都没省下。
        if let Some(value) = self.get_struct::<T>(key).await? {
            return Ok(value);
        }

        let value = loader().await?;
        self.set_struct(key, &value, ttl).await?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration as StdDuration;

    /// 注册表在回源结束后必须清掉该 key，否则每出现一个新 key 就永久多占一条记录。
    ///
    /// 断言只看**本测试自己的 key**：`FLIGHTS` 是进程级全局表，并行跑的其它测试
    /// 也在往里写，断言整表为空会随机失败。
    #[tokio::test]
    async fn flight_registry_is_cleaned_up() {
        let key = "cleanup-probe";
        assert!(!FLIGHTS.contains_key(key));
        {
            let _flight = begin_flight(key.to_string()).await;
            assert!(FLIGHTS.contains_key(key));
        }
        assert!(!FLIGHTS.contains_key(key), "回源结束后该 key 应被移除");
    }

    /// 同一 key 的并发回源必须串行；第二个任务要等第一个放手。
    #[tokio::test]
    async fn same_key_flights_are_serialized() {
        let running = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let running = running.clone();
                let max_seen = max_seen.clone();
                tokio::spawn(async move {
                    let _flight = begin_flight("hot".to_string()).await;
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(StdDuration::from_millis(5)).await;
                    running.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();

        for t in tasks {
            t.await.expect("任务不应 panic");
        }
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            1,
            "同一 key 同一时刻只能有一个回源者"
        );
        assert!(!FLIGHTS.contains_key("hot"));
    }

    /// 不同 key 之间不得互相阻塞。
    #[tokio::test]
    async fn different_keys_do_not_block_each_other() {
        let a = begin_flight("key-a".to_string()).await;
        // 另一个 key 必须立刻拿到，不能被 a 挡住
        let b = tokio::time::timeout(
            StdDuration::from_millis(100),
            begin_flight("key-b".to_string()),
        )
        .await
        .expect("不同 key 不应互相阻塞");

        assert!(FLIGHTS.contains_key("key-a") && FLIGHTS.contains_key("key-b"));
        drop(a);
        drop(b);
        assert!(!FLIGHTS.contains_key("key-a"));
        assert!(!FLIGHTS.contains_key("key-b"));
    }

    /// 回源者 panic / 被取消时，锁必须释放，key 不能永久卡死。
    #[tokio::test]
    async fn cancelled_flight_releases_the_key() {
        let task = tokio::spawn(async {
            let _flight = begin_flight("cancelled".to_string()).await;
            // 永远不结束，等着被取消
            std::future::pending::<()>().await;
        });
        // 让它先拿到锁
        tokio::time::sleep(StdDuration::from_millis(10)).await;
        task.abort();
        let _ = task.await;

        // 取消后应能立刻重新取得同一个 key
        tokio::time::timeout(
            StdDuration::from_millis(100),
            begin_flight("cancelled".to_string()),
        )
        .await
        .expect("被取消的回源必须释放锁");
    }
}
