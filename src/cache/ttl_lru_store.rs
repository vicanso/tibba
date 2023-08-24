use lru::LruCache;
use std::num::NonZeroUsize;
use tokio::sync::RwLock;

pub trait Expired {
    // 数据是否过期
    fn is_expired(&self) -> bool;
}

/// 基于LRU带有效期的存储组件
pub struct TtlLruStore<T> {
    // 如果直接使用lru，需要设置为mut
    // 而全局的mut会需要unsafe代码
    // 后续有了解到其它方案再调整
    // 带锁的lru实例
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
        // 性能考虑使用peek，但不会调整其顺序，因此热点数据也可能被清除
        // 由于其为ttl+lru，因此可设置更大的容量减少热点数据被清除
        if let Some(v) = cache.peek(key) {
            if !v.is_expired() {
                return Some(v.clone());
            }
        }
        None
    }
    pub async fn del(&mut self, key: &str) {
        let cache = &mut self.cache.write().await;
        cache.pop(key);
    }
}
