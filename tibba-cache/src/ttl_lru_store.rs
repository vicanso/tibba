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

use lru::LruCache;
use std::num::NonZeroUsize;
use tokio::sync::RwLock;

/// Trait for types that can expire
pub trait Expired {
    /// Checks if the data has expired
    /// # Returns
    /// * `true` - The data has expired and should be removed
    /// * `false` - The data is still valid
    fn is_expired(&self) -> bool;
}

/// A thread-safe storage component combining TTL (Time-To-Live) and LRU (Least Recently Used) caching strategies
/// # Type Parameters
/// * `T` - The type of values to store, must implement Expired trait
pub struct TtlLruStore<T> {
    /// Thread-safe LRU cache storing key-value pairs
    /// Uses RwLock for concurrent access with multiple readers or single writer
    cache: RwLock<LruCache<String, T>>,
}

impl<T: Expired + Clone> TtlLruStore<T> {
    /// Creates a new TtlLruStore with specified capacity
    /// # Arguments
    /// * `size` - Maximum number of items the cache can hold (must be non-zero)
    /// # Returns
    /// * A new TtlLruStore instance
    pub fn new(size: NonZeroUsize) -> Self {
        let cache: LruCache<String, T> = LruCache::new(size);
        TtlLruStore {
            cache: RwLock::new(cache),
        }
    }

    /// Stores a value in the cache
    /// # Arguments
    /// * `key` - The key under which to store the value
    /// * `value` - The value to store
    /// # Notes
    /// * If the cache is at capacity, the least recently used item will be removed
    /// * If the key already exists, the value will be updated
    pub async fn set(&self, key: &str, value: T) {
        let cache = &mut self.cache.write().await;
        cache.put(key.to_string(), value);
    }

    /// Retrieves a value from the cache if it exists and hasn't expired
    /// # Arguments
    /// * `key` - The key to look up
    /// # Returns
    /// * `Some(T)` - The value if found and not expired
    /// * `None` - If key doesn't exist or value has expired
    /// # Notes
    /// * Uses peek() instead of get() to avoid updating LRU order
    /// * Returns a clone of the value to maintain thread safety
    pub async fn get(&self, key: &str) -> Option<T> {
        let cache = self.cache.read().await;
        // better performance use peek to avoid moving the data to the front of the cache
        let v = cache.peek(key)?;
        if !v.is_expired() {
            return Some(v.clone());
        }
        None
    }

    /// Removes a value from the cache
    /// # Arguments
    /// * `key` - The key to remove
    /// # Notes
    /// * No-op if key doesn't exist
    /// * Requires mutable access to the store
    pub async fn del(&self, key: &str) {
        let mut cache = self.cache.write().await;
        cache.pop(key);
    }

    /// This method should be called periodically to free up memory from expired "garbage" data.
    pub async fn purge_expired(&self) {
        let mut cache = self.cache.write().await;
        // LruCache a a limited API for removal during iteration.
        // The safest way is to collect keys and then remove them.
        let keys_to_remove: Vec<String> = cache
            .iter()
            .filter(|(_, v)| v.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        if keys_to_remove.is_empty() {
            return;
        }

        for key in keys_to_remove {
            cache.pop(&key);
        }
    }
}
