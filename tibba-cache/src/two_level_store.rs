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
use chrono::Local;
use serde::{Serialize, de::DeserializeOwned};
use std::num::NonZeroUsize;
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

// Calculate expiration time aligned to interval boundaries
fn get_ttl_by_unit(unit: Duration) -> Duration {
    let now = Local::now();
    // Calculate the remaining time
    let seconds = unit.as_secs() - (now.timestamp() as u64 % unit.as_secs());
    // If less than 1/10 of the unit, extend the expiration time
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
impl<T: Clone + Serialize + DeserializeOwned> TwoLevelStore<T> {
    pub fn new(cache: RedisCache, size: NonZeroUsize, ttl: Duration) -> Self {
        TwoLevelStore {
            lru: TtlLruStore::new(size),
            ttl,
            redis: cache,
        }
    }
    pub async fn set(&self, key: &str, value: T) -> Result<()> {
        // Calculate the remaining time
        let ttl = get_ttl_by_unit(self.ttl);

        // Set redis cache first
        self.redis.set_struct(key, &value, Some(ttl)).await?;
        let data = ExpiredCache {
            data: value,
            expired_at: now() + ttl.as_secs() as i64,
        };

        self.lru.set(key, data).await;

        Ok(())
    }
    pub async fn get(&self, key: &str) -> Result<Option<T>> {
        // Read from lru first (get ensures it is not expired)
        if let Some(value) = self.lru.get(key).await {
            return Ok(Some(value.data));
        }
        let result: Option<T> = self.redis.get_struct(key).await?;
        // If there is a value, reset to lru
        if let Some(ref value) = result {
            let ttl = get_ttl_by_unit(self.ttl);
            // If ttl > self.ttl
            // It means the data may expire, so do not cache
            // Therefore, the scenario of caching ttl less than the default value
            if ttl <= self.ttl {
                let data = ExpiredCache {
                    data: value.clone(),
                    expired_at: now() + ttl.as_secs() as i64,
                };
                self.lru.set(key, data).await;
            }
        }
        Ok(result)
    }
}
