use chrono::Utc;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, slice::from_raw_parts, time::Duration};

use crate::error::HTTPResult;

use super::RedisCache;

pub trait Store {
    fn set<T>(&mut self, key: &str, value: &T, ttl: Duration) -> HTTPResult<()>
    where
        T: ?Sized + Serialize;
    fn get<'a, T>(&mut self, key: &str) -> HTTPResult<T>
    where
        T: Deserialize<'a>;
    fn del(&mut self, key: &str) -> HTTPResult<()>;
    fn close(&mut self) -> HTTPResult<()> {
        Ok(())
    }
}

// redis 实现的store存储
struct RedisStore {
    cache: RedisCache,
}
impl RedisStore {
    pub fn new(cache: RedisCache) -> Self {
        RedisStore { cache }
    }
}

impl Store for RedisStore {
    fn set<T>(&mut self, key: &str, value: &T, ttl: Duration) -> HTTPResult<()>
    where
        T: ?Sized + Serialize,
    {
        self.cache.set_struct(key, value, Some(ttl))
    }
    fn get<'a, T>(&mut self, key: &str) -> HTTPResult<T>
    where
        T: Deserialize<'a>,
    {
        self.cache.get_struct(key)
    }
    fn del(&mut self, key: &str) -> HTTPResult<()> {
        self.cache.del(key)
    }
}

pub struct LRUStore {
    cache: LruCache<String, Vec<u8>>,
}
impl LRUStore {
    pub fn new(size: usize) -> Self {
        let cache = LruCache::new(NonZeroUsize::new(size).unwrap());
        LRUStore { cache }
    }
}
impl Store for LRUStore {
    fn set<T>(&mut self, key: &str, value: &T, ttl: Duration) -> HTTPResult<()>
    where
        T: ?Sized + Serialize,
    {
        let mut value = serde_json::to_vec(&value)?;
        let cache = &mut self.cache;
        let expired = Utc::now().timestamp_nanos() + (ttl.as_nanos() as i64);

        for v in expired.to_be_bytes() {
            value.push(v);
        }
        cache.put(key.to_string(), value);
        Ok(())
    }
    fn get<'b, T>(&mut self, key: &str) -> HTTPResult<T>
    where
        T: Deserialize<'b>,
    {
        let mut value = vec![];
        let cache = &mut self.cache;

        if let Some(v) = cache.get(&key.to_string()) {
            // 获取8个字节
            let i64_size = 8;
            let size = v.len();
            if size > i64_size {
                let (left, right) = v.split_at(size - i64_size);
                let expired_arr = <[u8; 8]>::try_from(right).unwrap_or([0; 8]);
                let expired = i64::from_be_bytes(expired_arr);
                // 如果未过期
                if expired > Utc::now().timestamp_nanos() {
                    value = left.to_vec();
                }
            }
        }
        // TODO 生命周期是否有其它方法调整
        let result = unsafe {
            let p = value.as_ptr();
            serde_json::from_slice(from_raw_parts(p, value.len()))?
        };

        Ok(result)
    }
    fn del(&mut self, key: &str) -> HTTPResult<()> {
        let cache = &mut self.cache;
        cache.pop(&key.to_string());
        Ok(())
    }
}
