// Copyright 2025 Tree xie.
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

use super::{Error, Expired, RedisCache, TtlLruStore};
use serde::{Serialize, de::DeserializeOwned};
use std::num::NonZeroUsize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Error>;

#[inline]
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 计算与间隔边界对齐的 TTL，防止大量缓存在同一时刻集中失效（缓存雪崩）。
/// 若剩余时间不足 unit 的 1/10，则延伸至下一个间隔周期。
#[inline]
fn get_ttl_by_unit(unit: Duration) -> Duration {
    let secs = unit.as_secs();
    if secs == 0 {
        return Duration::ZERO;
    }
    // 计算距下一个间隔边界的剩余秒数
    let remaining = secs - (now_secs() % secs);
    // 剩余时间不足 1/10 时延伸到下一周期，避免刚写入就立刻过期
    if remaining < secs / 10 {
        return Duration::from_secs(secs + remaining);
    }
    Duration::from_secs(remaining)
}

/// 为缓存数据附加过期时间戳的包装结构体。
#[derive(Clone)]
struct ExpiredCache<T> {
    /// 实际缓存的数据
    data: T,
    /// 缓存条目的过期 Unix 时间戳（秒）
    expired_at: u64,
}

impl<T> Expired for ExpiredCache<T> {
    /// 当前时间已达到或超过 expired_at 时返回 `true`。
    fn is_expired(&self) -> bool {
        now_secs() >= self.expired_at
    }
}

/// 内存 LRU + Redis 双层缓存。
/// 读操作优先命中内存 LRU，未命中再查 Redis 并回填内存层。
pub struct TwoLevelStore<T> {
    /// 第一层：带 TTL 的内存 LRU 缓存
    lru: TtlLruStore<ExpiredCache<T>>,
    /// 缓存条目的默认 TTL
    ttl: Duration,
    /// 第二层：Redis 缓存
    redis: RedisCache,
}

impl<T: Clone + Serialize + DeserializeOwned> TwoLevelStore<T> {
    /// 创建新的 TwoLevelStore 实例。
    /// `size` 为内存 LRU 的最大条目数，`ttl` 为缓存默认过期时长。
    pub fn new(redis: RedisCache, size: NonZeroUsize, ttl: Duration) -> Self {
        Self {
            lru: TtlLruStore::new(size),
            ttl,
            redis,
        }
    }

    async fn fill_lru(&self, key: &str, value: T, ttl: Duration) {
        if ttl.is_zero() {
            return;
        }
        self.lru
            .set(
                key,
                ExpiredCache {
                    data: value,
                    expired_at: now_secs() + ttl.as_secs(),
                },
            )
            .await;
    }

    /// 将值写入两层缓存。
    /// TTL 与间隔边界对齐，先写 Redis 再更新内存 LRU。
    pub async fn set(&self, key: &str, value: T) -> Result<()> {
        // 计算对齐后的 TTL
        let ttl = get_ttl_by_unit(self.ttl);

        // 先写入 Redis，再更新内存缓存
        self.redis.set_struct(key, &value, Some(ttl)).await?;
        self.fill_lru(key, value, ttl).await;

        Ok(())
    }

    /// 从缓存读取值，优先查询内存 LRU，未命中则查 Redis 并回填 LRU。
    /// 若 Redis 中条目剩余 TTL 已超出预期范围则不回填内存层。
    pub async fn get(&self, key: &str) -> Result<Option<T>> {
        // 优先查内存 LRU（已过期条目不会被返回）
        if let Some(value) = self.lru.get(key).await {
            return Ok(Some(value.data));
        }
        // 内存未命中，查 Redis
        let result: Option<T> = self.redis.get_struct(key).await?;
        if let Some(value) = &result {
            let ttl = get_ttl_by_unit(self.ttl);
            // TTL 超出预期范围说明条目即将过期，不回填内存缓存
            if ttl <= self.ttl {
                self.fill_lru(key, value.clone(), ttl).await;
            }
        }
        Ok(result)
    }

    /// 从两层缓存中删除指定键。
    pub async fn del(&self, key: &str) -> Result<()> {
        self.lru.del(key).await;
        self.redis.del(key).await
    }

    /// 清除内存 LRU 中的过期条目，应定期调用以释放内存。
    /// Redis 侧的 TTL 由 Redis 自身管理，无需手动清理。
    pub async fn purge_expired(&self) {
        self.lru.purge_expired().await;
    }
}
