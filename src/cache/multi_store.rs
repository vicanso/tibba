use chrono::Utc;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::{num::NonZeroUsize, slice::from_raw_parts, sync::RwLock, time::Duration};

use super::RedisCache;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{}", source))]
    RedisClient { source: super::redis_client::Error },

    #[snafu(display("{}", message))]
    RwLock { message: String },
    #[snafu(display("Json fail: {}", source))]
    Json { source: serde_json::Error },
}
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<super::redis_client::Error> for Error {
    fn from(err: super::redis_client::Error) -> Self {
        Error::RedisClient { source: err }
    }
}

impl
    From<
        std::sync::PoisonError<
            std::sync::RwLockReadGuard<'_, lru::LruCache<std::string::String, std::vec::Vec<u8>>>,
        >,
    > for Error
{
    fn from(
        err: std::sync::PoisonError<
            std::sync::RwLockReadGuard<'_, lru::LruCache<std::string::String, std::vec::Vec<u8>>>,
        >,
    ) -> Self {
        Error::RwLock {
            message: err.to_string(),
        }
    }
}
impl
    From<
        std::sync::PoisonError<
            std::sync::RwLockWriteGuard<'_, lru::LruCache<std::string::String, std::vec::Vec<u8>>>,
        >,
    > for Error
{
    fn from(
        err: std::sync::PoisonError<
            std::sync::RwLockWriteGuard<'_, lru::LruCache<std::string::String, std::vec::Vec<u8>>>,
        >,
    ) -> Self {
        Error::RwLock {
            message: err.to_string(),
        }
    }
}

pub trait Store {
    fn set(&mut self, key: &str, value: Vec<u8>) -> Result<()>;
    fn get(&mut self, key: &str) -> Result<Vec<u8>>;
    fn del(&mut self, key: &str) -> Result<()>;
    fn close(&mut self) -> Result<()> {
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
    fn set(&mut self, key: &str, value: Vec<u8>) -> Result<()> {
        self.cache.set_bytes(key, value, Some(self.ttl))?;
        Ok(())
    }
    fn get(&mut self, key: &str) -> Result<Vec<u8>> {
        let result = self.cache.get_bytes(key)?;
        Ok(result)
    }
    fn del(&mut self, key: &str) -> Result<()> {
        self.cache.del(key)?;
        Ok(())
    }
}

pub struct TtlLruStore {
    cache: RwLock<LruCache<String, Vec<u8>>>,
    ttl: Duration,
}
impl TtlLruStore {
    pub fn new(size: usize, ttl: Duration) -> Self {
        let cache = LruCache::new(NonZeroUsize::new(size).unwrap());
        TtlLruStore {
            cache: RwLock::new(cache),
            ttl,
        }
    }
}
impl Store for TtlLruStore {
    fn set(&mut self, key: &str, value: Vec<u8>) -> Result<()> {
        let mut data = value;
        let cache = &mut self.cache.write()?;
        let expired = Utc::now().timestamp_nanos() + (self.ttl.as_nanos() as i64);

        for v in expired.to_be_bytes() {
            data.push(v);
        }
        cache.put(key.to_string(), data);
        Ok(())
    }
    fn get(&mut self, key: &str) -> Result<Vec<u8>> {
        let mut value = vec![];
        let cache = self.cache.read()?;
        // peek不会调整其顺序，因此热点数据也可能被清除
        // 由于其为ttl+lru，因此可设置更大的容量即可
        if let Some(v) = cache.peek(&key.to_string()) {
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
    fn del(&mut self, key: &str) -> Result<()> {
        let cache = &mut self.cache.write()?;
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
    pub fn set_struct<T>(&mut self, key: &str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).context(JsonSnafu {})?;
        for s in self.stores.iter_mut() {
            s.set(key, value.clone())?;
        }
        Ok(())
    }
    pub fn get_struct<'a, T>(&mut self, key: &str) -> Result<T>
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
            return Ok(T::default());
        }

        // TODO 生命周期是否有其它方法调整
        let result = unsafe {
            let p = value.as_ptr();
            serde_json::from_slice(from_raw_parts(p, value.len())).context(JsonSnafu {})?
        };

        Ok(result)
    }
}
