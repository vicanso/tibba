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

/// Calculates TTL (Time-To-Live) aligned to interval boundaries
/// # Arguments
/// * `unit` - The base duration unit to align with
/// # Returns
/// * Duration - Calculated TTL that aligns with the unit boundaries
/// # Notes
/// * If remaining time is less than 1/10 of unit, extends to next interval
/// * Helps prevent cache stampede by aligning expiration times
#[inline]
fn get_ttl_by_unit(unit: Duration) -> Duration {
    let secs = unit.as_secs();
    if secs == 0 {
        return Duration::ZERO;
    }
    // Calculate the remaining time until next interval
    let remaining = secs - (now_secs() % secs);
    // If less than 1/10 of the unit, extend to next interval
    if remaining < secs / 10 {
        return Duration::from_secs(secs + remaining);
    }
    Duration::from_secs(remaining)
}
/// Wrapper struct that adds expiration time to cached data
/// # Type Parameters
/// * `T` - The type of data being cached
#[derive(Clone)]
struct ExpiredCache<T> {
    /// The actual cached data
    data: T,
    /// Unix timestamp (seconds) when this cache entry expires
    expired_at: u64,
}

impl<T> Expired for ExpiredCache<T> {
    /// Checks if the cached data has expired
    /// # Returns
    /// * `true` - Cache entry has expired
    /// * `false` - Cache entry is still valid
    fn is_expired(&self) -> bool {
        now_secs() >= self.expired_at
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
        if let Some(value) = &result {
            let ttl = get_ttl_by_unit(self.ttl);
            // Only cache in LRU if TTL is within expected range
            // Prevents caching nearly-expired values
            if ttl <= self.ttl {
                self.fill_lru(key, value.clone(), ttl).await;
            }
        }
        Ok(result)
    }

    /// Removes a value from both cache levels
    /// # Arguments
    /// * `key` - The key to invalidate
    /// # Returns
    /// * `Ok(())` - Successfully removed from both caches
    /// * `Err(Error)` - Redis operation failed
    pub async fn del(&self, key: &str) -> Result<()> {
        self.lru.del(key).await;
        self.redis.del(key).await
    }

    /// Purges expired entries from the in-memory LRU cache
    /// # Notes
    /// * Should be called periodically to reclaim memory
    /// * Does not affect Redis entries (Redis handles its own TTL expiry)
    pub async fn purge_expired(&self) {
        self.lru.purge_expired().await;
    }
}
