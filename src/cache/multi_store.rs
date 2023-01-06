use chrono::Utc;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, slice::from_raw_parts, time::Duration};

use crate::error::HTTPResult;

use super::RedisCache;

pub trait Store {
    fn set(&mut self, key: &str, value: Vec<u8>) -> HTTPResult<()>;
    fn get(&mut self, key: &str) -> HTTPResult<Vec<u8>>;
    fn del(&mut self, key: &str) -> HTTPResult<()>;
    fn close(&mut self) -> HTTPResult<()> {
        Ok(())
    }
}

// redis 实现的store存储
pub struct TtlRedisStore {
    cache: RedisCache,
    ttl: Duration,
}
impl TtlRedisStore {
    pub fn new(cache: RedisCache, ttl: Duration) -> Self {
        TtlRedisStore { cache, ttl }
    }
}

impl Store for TtlRedisStore {
    fn set(&mut self, key: &str, value: Vec<u8>) -> HTTPResult<()> {
        self.cache.set_bytes(key, value, Some(self.ttl))
    }
    fn get(&mut self, key: &str) -> HTTPResult<Vec<u8>> {
        self.cache.get_bytes(key)
    }
    fn del(&mut self, key: &str) -> HTTPResult<()> {
        self.cache.del(key)
    }
}

pub struct TtlLruStore {
    cache: LruCache<String, Vec<u8>>,
    ttl: Duration,
}
impl TtlLruStore {
    pub fn new(size: usize, ttl: Duration) -> Self {
        let cache = LruCache::new(NonZeroUsize::new(size).unwrap());
        TtlLruStore { cache, ttl }
    }
}
impl Store for TtlLruStore {
    fn set(&mut self, key: &str, value: Vec<u8>) -> HTTPResult<()> {
        let mut data = value;
        let cache = &mut self.cache;
        let expired = Utc::now().timestamp_nanos() + (self.ttl.as_nanos() as i64);

        for v in expired.to_be_bytes() {
            data.push(v);
        }
        cache.put(key.to_string(), data);
        Ok(())
    }
    fn get(&mut self, key: &str) -> HTTPResult<Vec<u8>> {
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
        Ok(value)
    }
    fn del(&mut self, key: &str) -> HTTPResult<()> {
        let cache = &mut self.cache;
        cache.pop(&key.to_string());
        Ok(())
    }
}

pub struct TtlMultiStore {
    stores: Vec<Box<dyn Store>>,
}
impl TtlMultiStore {
    pub fn new(stores: Vec<Box<dyn Store>>) -> Self {
        TtlMultiStore { stores }
    }
    pub fn set_struct<T>(&mut self, key: &str, value: &T) -> HTTPResult<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value)?;
        for s in self.stores.iter_mut() {
            s.set(key, value.clone())?;
        }
        Ok(())
    }
    pub fn get_struct<'a, T>(&mut self, key: &str) -> HTTPResult<T>
    where
        T: Default + Deserialize<'a>,
    {
        let mut value: Vec<u8> = vec![];
        for s in self.stores.iter_mut() {
            let v = s.get(key)?;
            if !v.is_empty() {
                value = v;
                break;
            }
        }
        if value.is_empty() {
            return Ok(T::default())
        }
        
        // TODO 生命周期是否有其它方法调整
        let result = unsafe {
            let p = value.as_ptr();
            serde_json::from_slice(from_raw_parts(p, value.len()))?
        };

        Ok(result)
    }
}
