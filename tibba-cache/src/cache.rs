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

use super::{Error, RedisPool};
use deadpool_redis::redis::{cmd, pipe};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
use tibba_util::{lz4_decode, lz4_encode, zstd_decode, zstd_encode};

type Result<T> = std::result::Result<T, Error>;

pub struct RedisCache {
    ttl: Duration,
    prefix: String,
    pool: &'static RedisPool,
}

pub struct RedisCacheBuilder {
    ttl: Duration,
    prefix: String,
    pool: &'static RedisPool,
}

impl RedisCacheBuilder {
    pub fn new(pool: &'static RedisPool) -> Self {
        Self {
            ttl: Duration::from_secs(10 * 60),
            prefix: "".to_string(),
            pool,
        }
    }
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }
    pub fn prefix(mut self, prefix: String) -> Self {
        self.prefix = prefix;
        self
    }
    pub fn build(self) -> RedisCache {
        RedisCache {
            ttl: self.ttl,
            prefix: self.prefix,
            pool: self.pool,
        }
    }
}

impl RedisCache {
    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        self.prefix.to_string() + key
    }
    /// get value from redis
    async fn get_value<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let mut conn = self.pool.get().await?;
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
    /// set value to redis
    async fn set_value<T: redis::ToRedisArgs>(
        &self,
        key: &str,
        value: T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let mut conn = self.pool.get().await?;

        let seconds = ttl.unwrap_or(self.ttl).as_secs();
        let () = cmd("SETEX")
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
    /// Lock a key with a ttl, if the key is locked, return false
    /// if the key is not locked, set the key to true and return true
    pub async fn lock(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let mut conn = self.pool.get().await?;
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
    /// Delete a key from redis
    pub async fn del(&self, key: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let k = self.get_key(key);

        let () = cmd("DEL")
            .arg(&k)
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "del".to_string(),
                source: e,
            })?;
        Ok(())
    }
    /// Increment a key by delta.
    /// If the key does not exist, set the key to 0, the ttl of cache, and then increment by delta
    /// If the key exists, increment by delta.
    /// Return the new value of the key.
    pub async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        let mut conn = self.pool.get().await?;
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
            .query_async::<(bool, i64)>(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "incr".to_string(),
                source: e,
            })?;
        Ok(count)
    }
    // Set value to redis
    pub async fn set<T: redis::ToRedisArgs>(
        &self,
        key: &str,
        value: T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let k = self.get_key(key);
        self.set_value(&k, value, ttl).await
    }
    /// Get value from redis
    pub async fn get<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let k = self.get_key(key);
        self.get_value::<T>(&k).await
    }
    /// Set struct value to redis
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
    /// Get struct value from redis
    pub async fn get_struct<T>(&self, key: &str) -> Result<Option<T>>
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
    /// Get ttl from redis
    pub async fn ttl(&self, key: &str) -> Result<i32> {
        let mut conn = self.pool.get().await?;
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
    /// Get and delete a key from redis
    pub async fn get_del<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let k = self.get_key(key);
        let mut conn = self.pool.get().await?;
        let (value, _) = pipe()
            .cmd("GET")
            .arg(&k)
            .cmd("DEL")
            .arg(&k)
            .query_async::<(T, bool)>(&mut conn)
            .await
            .map_err(|e| Error::Redis {
                category: "get_del".to_string(),
                source: e,
            })?;
        Ok(value)
    }
    /// Set struct value to redis with lz4 compression
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
    /// Get struct value from redis with lz4 compression
    pub async fn get_struct_lz4<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let k = self.get_key(key);
        let value: Vec<u8> = self.get_value(&k).await?;

        if value.is_empty() {
            return Ok(None);
        }

        let buf = lz4_decode(value.as_slice()).map_err(|e| Error::Compression { source: e })?;

        let deserializer = &mut serde_json::Deserializer::from_slice(&buf);
        let result = T::deserialize(deserializer).map_err(|e| Error::Common {
            category: "get_struct_lz4".to_string(),
            message: e.to_string(),
        })?;
        Ok(Some(result))
    }
    /// Set struct value to redis with zstd compression
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
        let buf = zstd_encode(&value).map_err(|e| Error::Compression { source: e })?;
        let k = self.get_key(key);
        self.set_value(&k, &buf, ttl).await?;
        Ok(())
    }
    /// Get struct value from redis with zstd compression
    pub async fn get_struct_zstd<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let k = self.get_key(key);
        let value: Vec<u8> = self.get_value(&k).await?;

        if value.is_empty() {
            return Ok(None);
        }

        let buf = zstd_decode(value.as_slice()).map_err(|e| Error::Compression { source: e })?;

        let deserializer = &mut serde_json::Deserializer::from_slice(&buf);
        let result = T::deserialize(deserializer).map_err(|e| Error::Common {
            category: "get_struct_zstd".to_string(),
            message: e.to_string(),
        })?;
        Ok(Some(result))
    }
}
