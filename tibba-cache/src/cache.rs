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

use super::{Error, RedisClient, RedisClientConn};
use deadpool_redis::redis::{cmd, pipe};
use redis::{AsyncCommands, RedisError};
use serde::{Serialize, de::DeserializeOwned};
use std::{borrow::Cow, time::Duration};
use tibba_util::{Algorithm, compress, decompress};

type Result<T> = std::result::Result<T, Error>;

fn map_err(category: &str, e: RedisError) -> Error {
    Error::Redis {
        category: category.to_string(),
        source: e,
    }
}

/// Redis cache implementation that provides various caching operations
pub struct RedisCache {
    /// Time-to-live duration for cache entries
    ttl: Duration,
    /// Prefix added to all cache keys
    prefix: String,
    /// Redis connection pool
    client: &'static RedisClient,
}

impl RedisCache {
    #[inline]
    pub async fn conn(&self) -> Result<RedisClientConn> {
        self.client.conn().await
    }
    /// Creates a new RedisCacheBuilder with default settings:
    /// - TTL: 10 minutes
    /// - Empty prefix
    /// - Given Redis pool
    pub fn new(client: &'static RedisClient) -> Self {
        Self {
            ttl: Duration::from_secs(10 * 60),
            prefix: "".to_string(),
            client,
        }
    }

    /// Sets the time-to-live duration for cache entries
    /// Returns self for method chaining
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the prefix for all cache keys
    /// Returns self for method chaining
    pub fn with_prefix(mut self, prefix: String) -> Self {
        self.prefix = prefix;
        self
    }

    /// Generates the full cache key by combining prefix (if any) with the provided key
    /// # Arguments
    /// * `key` - The base key to be prefixed
    /// # Returns
    /// * If prefix is empty: returns the original key
    /// * If prefix exists: returns prefix + key
    fn get_key<'a>(&'a self, key: &'a str) -> Cow<'a, str> {
        if self.prefix.is_empty() {
            Cow::Borrowed(key)
        } else {
            Cow::Owned(format!("{}{}", self.prefix, key))
        }
    }
    /// Pings the Redis server to check connection
    /// # Returns
    /// * `Ok(())` - Connection is successful
    /// * `Err(Error)` - Redis operation failed
    pub async fn ping(&self) -> Result<()> {
        let () = self
            .conn()
            .await?
            .ping()
            .await
            .map_err(|e| map_err("ping", e))?;
        Ok(())
    }
    /// Retrieves a raw value from Redis for the given key
    /// # Type Parameters
    /// * `T` - The type to deserialize the Redis value into
    /// # Arguments
    /// * `key` - The key to retrieve
    /// # Returns
    /// * `Ok(T)` - Successfully retrieved and converted value
    /// * `Err(Error)` - Redis error or value conversion error
    async fn get_value<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let result = self
            .conn()
            .await?
            .get(key)
            .await
            .map_err(|e| map_err("get", e))?;

        Ok(result)
    }
    /// Stores a raw value in Redis with optional TTL
    /// # Type Parameters
    /// * `T` - The type of value to store, must be convertible to Redis data
    /// # Arguments
    /// * `key` - The key under which to store the value
    /// * `value` - The value to store
    /// * `ttl` - Optional time-to-live duration (uses instance default if None)
    async fn set_value<T: redis::ToRedisArgs + Send + Sync>(
        &self,
        key: &str,
        value: T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let seconds = ttl.unwrap_or(self.ttl).as_secs();
        let () = self
            .conn()
            .await?
            .set_ex(key, value, seconds)
            .await
            .map_err(|e| map_err("set", e))?;
        Ok(())
    }
    /// Attempts to acquire a distributed lock using Redis SET NX command
    /// # Arguments
    /// * `key` - The lock key
    /// * `ttl` - Optional lock duration (uses instance default if None)
    /// # Returns
    /// * `Ok(true)` - Lock was successfully acquired
    /// * `Ok(false)` - Lock already exists
    /// * `Err(Error)` - Redis operation failed
    pub async fn lock(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let mut conn = self.conn().await?;

        let result = cmd("SET")
            .arg(self.get_key(key))
            .arg(true)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .query_async(&mut conn)
            .await
            .map_err(|e| map_err("lock", e))?;
        Ok(result)
    }
    /// Removes a key and its value from Redis
    /// # Arguments
    /// * `key` - The key to delete
    /// # Returns
    /// * `Ok(())` - Key was successfully deleted (or didn't exist)
    /// * `Err(Error)` - Redis operation failed
    pub async fn del(&self, key: &str) -> Result<()> {
        let () = self
            .conn()
            .await?
            .del(self.get_key(key))
            .await
            .map_err(|e| map_err("del", e))?;

        Ok(())
    }
    /// Atomically increments a counter by delta
    /// # Arguments
    /// * `key` - The counter key
    /// * `delta` - Amount to increment by (can be negative)
    /// * `ttl` - Optional time-to-live for the counter
    /// # Returns
    /// * `Ok(i64)` - The new value after incrementing
    /// * `Err(Error)` - Redis operation failed
    /// # Notes
    /// If the key doesn't exist, it's initialized to 0 with ttl before incrementing
    pub async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        let mut conn = self.conn().await?;
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
            .map_err(|e| map_err("incr", e))?;
        Ok(count)
    }
    /// Sets a value in Redis with an optional TTL
    /// - If TTL is None, uses the default TTL configured for this cache
    /// - Value type must implement ToRedisArgs trait
    /// - Key will be automatically prefixed if a prefix is configured
    pub async fn set<T: redis::ToRedisArgs + Send + Sync>(
        &self,
        key: &str,
        value: T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        self.set_value(&self.get_key(key), value, ttl).await
    }
    /// Retrieves a value from Redis
    /// - Value type must implement FromRedisValue trait
    /// - Key will be automatically prefixed if a prefix is configured
    /// - Returns Error if key doesn't exist or value can't be converted to T
    pub async fn get<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        self.get_value::<T>(&self.get_key(key)).await
    }
    /// Serializes and stores a struct in Redis as JSON
    /// - Value must implement Serialize trait
    /// - Optional TTL (uses default if None)
    /// - Key will be automatically prefixed
    pub async fn set_struct<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).map_err(|e| Error::Common {
            category: "set_struct".to_string(),
            message: e.to_string(),
        })?;
        self.set_value(&self.get_key(key), &value, ttl).await?;
        Ok(())
    }
    /// Retrieves and deserializes a struct from Redis
    /// - Type must implement DeserializeOwned trait
    /// - Returns None if key doesn't exist
    /// - Returns Error if deserialization fails
    /// - Key will be automatically prefixed
    pub async fn get_struct<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let buf: Vec<u8> = self.get_value(&self.get_key(key)).await?;

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
    /// Gets the remaining time-to-live for a key
    /// # Arguments
    /// * `key` - The key to check
    /// # Returns
    /// * `Ok(seconds)` where:
    ///   * `seconds > 0` - Remaining time in seconds
    ///   * `seconds = -2` - Key does not exist
    ///   * `seconds = -1` - Key exists but has no expiry
    /// * `Err(Error)` - Redis operation failed
    pub async fn ttl(&self, key: &str) -> Result<i32> {
        let result = self
            .conn()
            .await?
            .ttl(self.get_key(key))
            .await
            .map_err(|e| map_err("ttl", e))?;

        Ok(result)
    }
    /// Atomically retrieves a value and deletes it from Redis(>=6.2.0)
    /// # Type Parameters
    /// * `T` - The type to deserialize the Redis value into
    /// # Arguments
    /// * `key` - The key to get and delete
    /// # Returns
    /// * `Ok(T)` - The value before deletion
    /// * `Err(Error)` - Redis operation failed or value conversion error
    pub async fn get_del<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let result = self
            .conn()
            .await?
            .get_del(self.get_key(key))
            .await
            .map_err(|e| map_err("get_del", e))?;

        Ok(result)
    }
    async fn set_struct_compressed<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
        algorithm: Algorithm,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(value).map_err(|e| Error::Common {
            category: "serde_json".to_string(),
            message: e.to_string(),
        })?;
        let buf = compress(&value, algorithm).map_err(|e| Error::Compression { source: e })?;
        self.set_value(&self.get_key(key), &buf, ttl).await
    }

    async fn get_struct_compressed<T>(&self, key: &str, algorithm: Algorithm) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let value: Option<Vec<u8>> = self.get_value(&self.get_key(key)).await?;
        match value {
            None => Ok(None),
            Some(compressed_buf) => {
                let buf = decompress(&compressed_buf, algorithm)
                    .map_err(|e| Error::Compression { source: e })?;
                serde_json::from_slice(&buf)
                    .map_err(|e| Error::Common {
                        category: "serde_json".to_string(),
                        message: e.to_string(),
                    })
                    .map(Some)
            }
        }
    }
    /// Serializes a struct to JSON, compresses it with LZ4, and stores in Redis
    /// # Type Parameters
    /// * `T` - The struct type to serialize
    /// # Arguments
    /// * `key` - The key under which to store the compressed data
    /// * `value` - The struct to serialize and compress
    /// * `ttl` - Optional time-to-live duration
    /// # Notes
    /// Uses LZ4 compression which favors speed over compression ratio
    pub async fn set_struct_lz4<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        self.set_struct_compressed(key, value, ttl, Algorithm::Lz4)
            .await
    }
    /// Retrieves, decompresses (LZ4), and deserializes a struct from Redis
    /// # Type Parameters
    /// * `T` - The struct type to deserialize into
    /// # Arguments
    /// * `key` - The key to retrieve
    /// # Returns
    /// * `Ok(Some(T))` - Successfully retrieved and deserialized value
    /// * `Ok(None)` - Key doesn't exist
    /// * `Err(Error)` - Redis, decompression, or deserialization error
    pub async fn get_struct_lz4<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.get_struct_compressed(key, Algorithm::Lz4).await
    }
    /// Serializes a struct to JSON, compresses it with Zstd, and stores in Redis
    /// # Type Parameters
    /// * `T` - The struct type to serialize
    /// # Arguments
    /// * `key` - The key under which to store the compressed data
    /// * `value` - The struct to serialize and compress
    /// * `ttl` - Optional time-to-live duration
    /// # Notes
    /// Uses Zstd compression which provides better compression ratios than LZ4
    pub async fn set_struct_zstd<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        self.set_struct_compressed(key, value, ttl, Algorithm::default())
            .await
    }
    /// Retrieves, decompresses (Zstd), and deserializes a struct from Redis
    /// # Type Parameters
    /// * `T` - The struct type to deserialize into
    /// # Arguments
    /// * `key` - The key to retrieve
    /// # Returns
    /// * `Ok(Some(T))` - Successfully retrieved and deserialized value
    /// * `Ok(None)` - Key doesn't exist
    /// * `Err(Error)` - Redis, decompression, or deserialization error
    pub async fn get_struct_zstd<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.get_struct_compressed(key, Algorithm::default()).await
    }
}
