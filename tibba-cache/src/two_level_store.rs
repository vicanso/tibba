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
use chrono::Utc;
use serde::{Serialize, de::DeserializeOwned};
use std::num::NonZeroUsize;
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

/// Calculates TTL (Time-To-Live) aligned to interval boundaries
/// # Arguments
/// * `unit` - The base duration unit to align with
/// # Returns
/// * Duration - Calculated TTL that aligns with the unit boundaries
/// # Notes
/// * If remaining time is less than 1/10 of unit, extends to next interval
/// * Helps prevent cache stampede by aligning expiration times
fn get_ttl_by_unit(unit: Duration) -> Duration {
    let now = Utc::now();
    // Calculate the remaining time until next interval
    let seconds = unit.as_secs() - (now.timestamp() as u64 % unit.as_secs());
    // If less than 1/10 of the unit, extend to next interval
    if seconds < unit.as_secs() / 10 {
        return Duration::from_secs(unit.as_secs() + seconds);
    }
    Duration::from_secs(seconds)
}

/// Gets current timestamp in seconds
/// # Returns
/// * `i64` - Current Unix timestamp
fn now_utc() -> i64 {
    Utc::now().timestamp()
}
/// Wrapper struct that adds expiration time to cached data
/// # Type Parameters
/// * `T` - The type of data being cached
#[derive(Clone)]
struct ExpiredCache<T> {
    /// The actual cached data
    data: T,
    /// Unix timestamp when this cache entry expires
    expired_at: i64,
}

impl<T> Expired for ExpiredCache<T> {
    /// Checks if the cached data has expired
    /// # Returns
    /// * `true` - Cache entry has not expired yet
    /// * `false` - Cache entry has expired
    fn is_expired(&self) -> bool {
        now_utc() >= self.expired_at
    }
}

/// Two-level cache implementation combining in-memory LRU cache and Redis
/// # Type Parameters
/// * `T` - The type of data being cached
pub struct TwoLevelStore<T> {
    /// First level: In-memory LRU cache with TTL
    lru: TtlLruStore<ExpiredCache<T>>,
    /// Default TTL for cache entries
    ttl: Duration,
    /// Second level: Redis cache
    redis: RedisCache,
}

impl<T: Clone + Serialize + DeserializeOwned> TwoLevelStore<T> {
    /// Creates a new TwoLevelStore instance
    /// # Arguments
    /// * `cache` - Redis cache instance for second level storage
    /// * `size` - Maximum number of entries in the LRU cache
    /// * `ttl` - Default time-to-live for cache entries
    /// # Returns
    /// * New TwoLevelStore instance
    pub fn new(cache: RedisCache, size: NonZeroUsize, ttl: Duration) -> Self {
        TwoLevelStore {
            lru: TtlLruStore::new(size),
            ttl,
            redis: cache,
        }
    }
    async fn fill_lru(&self, key: &str, value: T, ttl: Duration) {
        if ttl.is_zero() {
            return;
        }
        let expired_at = now_utc() + ttl.as_secs() as i64;
        let data = ExpiredCache {
            data: value,
            expired_at,
        };
        self.lru.set(key, data).await;
    }

    /// Stores a value in both cache levels
    /// # Arguments
    /// * `key` - The key under which to store the value
    /// * `value` - The value to store
    /// # Returns
    /// * `Ok(())` - Successfully stored in both caches
    /// * `Err(Error)` - Failed to store in Redis
    /// # Notes
    /// * Calculates TTL aligned with interval boundaries
    /// * Stores in Redis first, then LRU cache
    pub async fn set(&self, key: &str, value: T) -> Result<()> {
        // Calculate the remaining time
        let ttl = get_ttl_by_unit(self.ttl);

        // Set redis cache first
        self.redis.set_struct(key, &value, Some(ttl)).await?;
        self.fill_lru(key, value, ttl).await;

        Ok(())
    }

    /// Retrieves a value from the cache
    /// # Arguments
    /// * `key` - The key to look up
    /// # Returns
    /// * `Ok(Some(T))` - Value found in either cache level
    /// * `Ok(None)` - Value not found in either cache
    /// * `Err(Error)` - Redis operation failed
    /// # Notes
    /// * Checks LRU cache first
    /// * If not in LRU, checks Redis and updates LRU if found
    /// * Only updates LRU if Redis TTL is within expected range
    pub async fn get(&self, key: &str) -> Result<Option<T>> {
        // Try LRU cache first (get ensures it is not expired)
        if let Some(value) = self.lru.get(key).await {
            return Ok(Some(value.data));
        }
        // Try Redis if not in LRU
        let result: Option<T> = self.redis.get_struct(key).await?;
        // If found in Redis, potentially update LRU
        if let Some(ref value) = result {
            let ttl = get_ttl_by_unit(self.ttl);
            // Only cache in LRU if TTL is within expected range
            // Prevents caching nearly-expired values
            if ttl <= self.ttl {
                self.fill_lru(key, value.clone(), ttl).await;
            }
        }
        Ok(result)
    }
}
