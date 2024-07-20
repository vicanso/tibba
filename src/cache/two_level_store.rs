use super::{Expired, RedisCache, Result, TtlLruStore};
use chrono::Local;
use serde::{de::DeserializeOwned, Serialize};
use std::num::NonZeroUsize;
use std::time::Duration;

// 根据当前时间以及unit计算有效期，让有效期尽可能落在间隔点
fn get_ttl_by_unit(unit: Duration) -> Duration {
    let now = Local::now();
    // 计算剩下的时间
    let seconds = unit.as_secs() - (now.timestamp() as u64 % unit.as_secs());
    // 如果少于1/10
    if seconds < unit.as_secs() / 10 {
        return Duration::from_secs(unit.as_secs() + seconds);
    }
    Duration::from_secs(seconds)
}

fn now() -> i64 {
    Local::now().timestamp()
}

#[derive(Clone)]
struct ExpiredCache<T> {
    data: T,
    expired_at: i64,
}
impl<T> Expired for ExpiredCache<T> {
    fn is_expired(&self) -> bool {
        now() < self.expired_at
    }
}

pub struct TwoLevelStore<T> {
    lru: TtlLruStore<ExpiredCache<T>>,
    ttl: Duration,
    redis: RedisCache,
}
impl<T: Clone + ?Sized + Serialize + DeserializeOwned> TwoLevelStore<T> {
    pub fn new(size: NonZeroUsize, ttl: Duration, prefix: String) -> Self {
        TwoLevelStore {
            lru: TtlLruStore::new(size),
            ttl,
            redis: RedisCache::new_with_ttl_prefix(ttl, prefix),
        }
    }
    pub async fn set(&self, key: &str, value: T) -> Result<()> {
        // 根据ttl按区间重新计算值
        let ttl = get_ttl_by_unit(self.ttl);

        // 先设置redis缓存
        self.redis.set_struct(key, &value, Some(ttl)).await?;

        self.lru
            .set(
                key,
                ExpiredCache {
                    data: value,
                    expired_at: now() + ttl.as_secs() as i64,
                },
            )
            .await;

        Ok(())
    }
    pub async fn get(&self, key: &str) -> Result<Option<T>> {
        // 从先lru读取(get保证了肯定不过期)
        if let Some(value) = self.lru.get(key).await {
            return Ok(Some(value.data));
        }
        let result: Option<T> = self.redis.get_struct(key).await?;
        // 如果有值，重新设置至lru
        if let Some(ref value) = result {
            let ttl = get_ttl_by_unit(self.ttl);
            // 如果ttl > self.ttl
            // 则表示数据可能要过期，不缓存
            // 因此缓存ttl少于默认值的场景
            if ttl <= self.ttl {
                self.lru
                    .set(
                        key,
                        ExpiredCache {
                            data: value.clone(),
                            expired_at: now() + ttl.as_secs() as i64,
                        },
                    )
                    .await;
            }
        }
        Ok(result)
    }
}
