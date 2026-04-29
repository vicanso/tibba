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

/// 用于判断缓存数据是否已过期的 trait。
pub trait Expired {
    /// 返回 `true` 表示数据已过期，应从缓存中移除。
    fn is_expired(&self) -> bool;
}

/// 线程安全的 TTL + LRU 两级淘汰缓存存储。
/// 同时支持多读或单写并发访问。
pub struct TtlLruStore<T> {
    cache: RwLock<LruCache<String, T>>,
}

impl<T: Expired + Clone> TtlLruStore<T> {
    /// 创建指定容量的 TtlLruStore，容量必须大于 0。
    pub fn new(size: NonZeroUsize) -> Self {
        Self {
            cache: RwLock::new(LruCache::new(size)),
        }
    }

    /// 向缓存写入键值对。容量已满时自动淘汰最久未使用的条目。
    pub async fn set(&self, key: &str, value: T) {
        let mut cache = self.cache.write().await;
        cache.put(key.to_string(), value);
    }

    /// 读取未过期的缓存值，键不存在或已过期时返回 `None`。
    /// 内部使用 peek 而非 get，不更新 LRU 顺序，性能更优。
    pub async fn get(&self, key: &str) -> Option<T> {
        let cache = self.cache.read().await;
        // 使用 peek 避免更新 LRU 顺序，读多写少场景性能更佳
        cache.peek(key).filter(|v| !v.is_expired()).cloned()
    }

    /// 删除指定键，键不存在时为空操作。
    pub async fn del(&self, key: &str) {
        let mut cache = self.cache.write().await;
        cache.pop(key);
    }

    /// 清除所有已过期的条目，应定期调用以释放内存。
    pub async fn purge_expired(&self) {
        let mut cache = self.cache.write().await;
        // LruCache 不支持迭代中删除，需先收集过期键再批量移除
        let keys: Vec<String> = cache
            .iter()
            .filter(|(_, v)| v.is_expired())
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            cache.pop(&key);
        }
    }
}
