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

pub trait Expired {
    // check if the data is expired
    fn is_expired(&self) -> bool;
}

/// ttl+lru based storage component
pub struct TtlLruStore<T> {
    cache: RwLock<LruCache<String, T>>,
}
impl<T: Expired + Clone> TtlLruStore<T> {
    pub fn new(size: NonZeroUsize) -> Self {
        let cache: LruCache<String, T> = LruCache::new(size);
        TtlLruStore {
            cache: RwLock::new(cache),
        }
    }
    pub async fn set(&self, key: &str, value: T) {
        let cache = &mut self.cache.write().await;
        cache.put(key.to_string(), value);
    }
    pub async fn get(&self, key: &str) -> Option<T> {
        let cache = self.cache.read().await;
        // better performance use peek to avoid moving the data to the front of the cache
        let v = cache.peek(key)?;
        if !v.is_expired() {
            return Some(v.clone());
        }
        None
    }
    pub async fn del(&mut self, key: &str) {
        let cache = &mut self.cache.write().await;
        cache.pop(key);
    }
}
