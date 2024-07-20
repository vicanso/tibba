use super::redis_pool::{must_get_redis_connection, RedisConnection};
use super::{Error, Result};
use crate::util::{lz4_decode, lz4_encode, zstd_decode, zstd_encode};
use deadpool_redis::redis::{cmd, pipe};
use once_cell::sync::OnceCell;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

pub async fn redis_ping() -> Result<String> {
    let mut conn = must_get_redis_connection().await?;
    cmd("PING")
        .query_async::<RedisConnection, String>(&mut conn)
        .await
        .map_err(|e| Error::Redis {
            category: "ping".to_string(),
            source: e,
        })
}

#[derive(Default, Clone, Debug)]
pub struct RedisCache {
    ttl: Duration,
    prefix: String,
}

/// 获取默认的redis缓存，基于redis pool并设置默认的ttl
pub fn get_default_redis_cache() -> &'static RedisCache {
    static DEFAULT_REDIS_CACHE: OnceCell<RedisCache> = OnceCell::new();
    DEFAULT_REDIS_CACHE.get_or_init(RedisCache::new)
}

impl RedisCache {
    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        self.prefix.to_string() + key
    }
    /// 从redis中获取数据
    async fn get_value<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let mut conn = must_get_redis_connection().await?;
        let result = cmd("GET")
            .arg(key)
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "get".to_string(),
                source: e,
            })?;

        Ok(result)
    }
    /// 将数据设置至redis中，如果未设置ttl则使用默认值
    async fn set_value<T: redis::ToRedisArgs>(
        &self,
        key: &str,
        value: T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let mut conn = must_get_redis_connection().await?;

        let seconds = ttl.unwrap_or(self.ttl).as_secs();
        cmd("SETEX")
            .arg(key)
            .arg(seconds)
            .arg(value)
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "set".to_string(),
                source: e,
            })?;
        Ok(())
    }
    /// 使用默认的ttl初始化redis缓存实例
    pub fn new() -> RedisCache {
        Self::new_with_ttl(Duration::from_secs(5 * 60))
    }
    /// 初始化redis缓存实例，并指定默认的ttl
    pub fn new_with_ttl(ttl: Duration) -> RedisCache {
        RedisCache {
            ttl,
            ..Default::default()
        }
    }
    /// 初始化redis缓存实例，指定ttl以及prefix
    pub fn new_with_ttl_prefix(ttl: Duration, prefix: String) -> RedisCache {
        RedisCache { ttl, prefix }
    }
    /// 尝试锁定key，时长为ttl，若未指定时长则使用默认时长
    /// 如果成功则返回true，否则返回false。
    /// 主要用于多实例并发限制。
    pub async fn lock(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let mut conn = must_get_redis_connection().await?;
        let k = self.get_key(key);

        let result = cmd("SET")
            .arg(&k)
            .arg(true)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "lock".to_string(),
                source: e,
            })?;
        Ok(result)
    }
    /// 从redis中删除key
    pub async fn del(&self, key: &str) -> Result<()> {
        let mut conn = must_get_redis_connection().await?;
        let k = self.get_key(key);

        cmd("DEL")
            .arg(&k)
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "del".to_string(),
                source: e,
            })?;
        Ok(())
    }
    /// 增加redis中key所对应的值，如果ttl未指定则使用默认值，
    /// 需要注意此ttl仅在首次时设置。
    pub async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        let mut conn = must_get_redis_connection().await?;
        let k = self.get_key(key);
        let (_, count) = pipe()
            .cmd("SET")
            .arg(&k)
            .arg(0)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .cmd("INCRBY")
            .arg(&k)
            .arg(delta)
            .query_async::<RedisConnection, (bool, i64)>(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "incr".to_string(),
                source: e,
            })?;
        Ok(count)
    }

    /// 将数据设置至redis中，如果未设置ttl则使用默认值
    pub async fn set<T: redis::ToRedisArgs>(
        &self,
        key: &str,
        value: T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let k = self.get_key(key);
        self.set_value(&k, value, ttl).await
    }

    /// 从redis中获取数据
    pub async fn get<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let k = self.get_key(key);
        self.get_value::<T>(&k).await
    }
    /// 将struct转换为json后设置至redis中，若未指定ttl则使用默认值
    pub async fn set_struct<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).map_err(|e| Error::Common {
            category: "set_struct".to_string(),
            message: e.to_string(),
        })?;
        let k = self.get_key(key);
        self.set_value(&k, &value, ttl).await?;
        Ok(())
    }
    /// 从redis中获取数据并转换为struct，如果缓存中无数据则返回None
    pub async fn get_struct<'a, T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let k = self.get_key(key);
        let buf: Vec<u8> = self.get_value(&k).await?;

        if buf.is_empty() {
            return Ok(None);
        }

        let deserializer = &mut serde_json::Deserializer::from_slice(&buf);
        let result = T::deserialize(deserializer).map_err(|e| Error::Common {
            category: "get_struct".to_string(),
            message: e.to_string(),
        })?;

        Ok(Some(result))
    }
    /// 返回该key在redis中的有效期
    pub async fn ttl(&self, key: &str) -> Result<i32> {
        let mut conn = must_get_redis_connection().await?;
        let k = self.get_key(key);
        let result = cmd("TTL")
            .arg(&k)
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "ttl".to_string(),
                source: e,
            })?;
        Ok(result)
    }
    /// 获取后并删除该key在redis中的值，用于仅获取一次的场景
    pub async fn get_del<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let k = self.get_key(key);
        let mut conn = must_get_redis_connection().await?;
        let (value, _) = pipe()
            .cmd("GET")
            .arg(&k)
            .cmd("DEL")
            .arg(&k)
            .query_async::<RedisConnection, (T, bool)>(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "get_del".to_string(),
                source: e,
            })?;
        Ok(value)
    }
    /// 将struct转换为json后使用lz4压缩，
    /// 再将压缩后的数据设置至redis中，若未指定ttl，
    /// 则使用默认的有效期
    pub async fn set_struct_lz4<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).map_err(|e| Error::Common {
            category: "set_struct_lz4".to_string(),
            message: e.to_string(),
        })?;
        let buf = lz4_encode(&value);
        let k = self.get_key(key);
        self.set_value(&k, &buf, ttl).await?;
        Ok(())
    }
    /// 从redis获取数据后使用lz4解压，并转换为对应的struct
    pub async fn get_struct_lz4<'a, T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let k = self.get_key(key);
        let value: Vec<u8> = self.get_value(&k).await?;

        if value.is_empty() {
            return Ok(None);
        }

        let buf = lz4_decode(value.as_slice()).map_err(|e| Error::Compress { source: e })?;

        let deserializer = &mut serde_json::Deserializer::from_slice(&buf);
        let result = T::deserialize(deserializer).map_err(|e| Error::Common {
            category: "get_struct_lz4".to_string(),
            message: e.to_string(),
        })?;
        Ok(Some(result))
    }
    /// 将struct转换为json后使用zstd压缩，
    /// 再将压缩后的数据设置至redis中，若未指定ttl，
    /// 则使用默认的有效期
    pub async fn set_struct_zstd<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).map_err(|e| Error::Common {
            category: "set_struct_zstd".to_string(),
            message: e.to_string(),
        })?;
        let buf = zstd_encode(&value).map_err(|e| Error::Compress { source: e })?;
        let k = self.get_key(key);
        self.set_value(&k, &buf, ttl).await?;
        Ok(())
    }
    /// 从redis获取数据后使用zstd解压，并转换为对应的struct
    pub async fn get_struct_zstd<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let k = self.get_key(key);
        let value: Vec<u8> = self.get_value(&k).await?;

        if value.is_empty() {
            return Ok(None);
        }

        let buf = zstd_decode(value.as_slice()).map_err(|e| Error::Compress { source: e })?;

        let deserializer = &mut serde_json::Deserializer::from_slice(&buf);
        let result = T::deserialize(deserializer).map_err(|e| Error::Common {
            category: "get_struct_zstd".to_string(),
            message: e.to_string(),
        })?;
        Ok(Some(result))
    }
}
