use lru::LruCache;
use snafu::Snafu;
use std::{num::NonZeroUsize, sync::RwLock};

use crate::error::HTTPError;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{category} {message}"))]
    RwLock { category: String, message: String },
    #[snafu(display("Json fail, {source}"))]
    Json { source: serde_json::Error },
}
pub type Result<T, E = Error> = std::result::Result<T, E>;
// 实现对http error的转换
impl From<Error> for HTTPError {
    fn from(err: Error) -> Self {
        // 对于部分error单独转换
        match err {
            Error::RwLock { message, category } => {
                let cat = format!("rwLock:{category}");
                HTTPError::new_with_category(&message, cat.as_str())
            }
            _ => HTTPError::new_with_category(&err.to_string(), "multiStore"),
        }
    }
}

pub trait Expired {
    // 数据是否过期
    fn is_expired(&self) -> bool;
}

/// 基于LRU带有效期的存储组件
pub struct TtlLruStore<T> {
    // 带锁的lru实例
    cache: RwLock<LruCache<String, T>>,
}
impl<T: Expired + Clone> TtlLruStore<T> {
    pub fn new(size: usize) -> Self {
        let cache: LruCache<String, T> = LruCache::new(NonZeroUsize::new(size).unwrap());
        TtlLruStore {
            cache: RwLock::new(cache),
        }
    }
    async fn set(&mut self, key: &str, value: T) -> Result<()> {
        let cache = &mut self.cache.write().map_err(|err| Error::RwLock {
            message: err.to_string(),
            category: "write".to_string(),
        })?;
        cache.put(key.to_string(), value);
        Ok(())
    }
    async fn get(&mut self, key: &str) -> Result<Option<T>> {
        let cache = self.cache.read().map_err(|err| Error::RwLock {
            message: err.to_string(),
            category: "read".to_string(),
        })?;
        // 性能考虑使用peek，但不会调整其顺序，因此热点数据也可能被清除
        // 由于其为ttl+lru，因此可设置更大的容量减少热点数据被清除
        if let Some(v) = cache.peek(key) {
            if !v.is_expired() {
                return Ok(Some(v.clone()));
            }
        }
        Ok(None)
    }
    async fn del(&mut self, key: &str) -> Result<()> {
        let cache = &mut self.cache.write().map_err(|err| Error::RwLock {
            message: err.to_string(),
            category: "write".to_string(),
        })?;
        cache.pop(key);
        Ok(())
    }
}
