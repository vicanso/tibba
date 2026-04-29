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

use super::{CompressionSnafu, Error, RedisClient, RedisClientConn, RedisSnafu, SerdeJsonSnafu};
use deadpool_redis::redis::{cmd, pipe};
use redis::AsyncCommands;
use serde::{Serialize, de::DeserializeOwned};
use snafu::ResultExt;
use std::{borrow::Cow, time::Duration};
use tibba_util::{Algorithm, compress, decompress};

const DEFAULT_ZSTD: Algorithm = Algorithm::Zstd(3);

type Result<T> = std::result::Result<T, Error>;

/// Redis 缓存封装，提供键值读写、分布式锁、计数器等常用缓存操作。
pub struct RedisCache {
    /// 缓存条目的默认过期时长
    ttl: Duration,
    /// 所有缓存键统一添加的前缀
    prefix: String,
    /// Redis 连接池
    client: &'static RedisClient,
}

impl RedisCache {
    #[inline]
    pub async fn conn(&self) -> Result<RedisClientConn> {
        self.client.conn().await
    }

    /// 创建新的 RedisCache 实例，默认 TTL 10 分钟，无前缀。
    pub fn new(client: &'static RedisClient) -> Self {
        Self {
            ttl: Duration::from_secs(10 * 60),
            prefix: String::new(),
            client,
        }
    }

    /// 设置缓存条目的过期时长，支持链式调用。
    #[must_use]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// 设置所有缓存键的前缀，支持链式调用。
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    #[inline]
    fn get_ttl(&self, ttl: Option<Duration>) -> u64 {
        ttl.unwrap_or(self.ttl).as_secs()
    }

    /// 拼接前缀与键名，生成完整的缓存键。
    /// 前缀为空时直接借用原始键，避免额外分配。
    #[inline]
    fn get_key<'a>(&'a self, key: &'a str) -> Cow<'a, str> {
        if self.prefix.is_empty() {
            Cow::Borrowed(key)
        } else {
            Cow::Owned(format!("{}{}", self.prefix, key))
        }
    }

    /// 向 Redis 发送 PING 以检测连接是否正常。
    pub async fn ping(&self) -> Result<()> {
        let () = self
            .conn()
            .await?
            .ping()
            .await
            .context(RedisSnafu { category: "ping" })?;
        Ok(())
    }

    /// 从 Redis 读取原始值，类型由调用方通过泛型指定。
    async fn get_value<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let result = self
            .conn()
            .await?
            .get(key)
            .await
            .context(RedisSnafu { category: "get" })?;

        Ok(result)
    }

    /// 向 Redis 写入原始值，并设置过期时间（秒）。
    async fn set_value<T: redis::ToSingleRedisArg + Send + Sync>(
        &self,
        key: &str,
        value: T,
        ttl: u64,
    ) -> Result<()> {
        let () = self
            .conn()
            .await?
            .set_ex(key, value, ttl)
            .await
            .context(RedisSnafu { category: "set" })?;
        Ok(())
    }

    /// 尝试通过 SET NX 获取分布式锁。
    /// 返回 `true` 表示加锁成功，`false` 表示锁已被持有。
    pub async fn lock(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let mut conn = self.conn().await?;

        let result = cmd("SET")
            .arg(self.get_key(key))
            .arg(true)
            .arg("NX")
            .arg("EX")
            .arg(self.get_ttl(ttl))
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "lock" })?;
        Ok(result)
    }

    /// 删除指定键。
    pub async fn del(&self, key: &str) -> Result<()> {
        let () = self
            .conn()
            .await?
            .del(self.get_key(key))
            .await
            .context(RedisSnafu { category: "del" })?;

        Ok(())
    }

    /// 原子性地将计数器累加 delta，返回累加后的值。
    /// 键不存在时先用 SET NX 初始化为 0 再执行 INCRBY。
    pub async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        let mut conn = self.conn().await?;
        let k = self.get_key(key);
        // 这里的逻辑逻辑更加自然
        let (count, _) = pipe()
            .cmd("INCRBY")
            .arg(&k)
            .arg(delta) // 1. 先累加（不存在会自动创建，且无 TTL）
            .cmd("EXPIRE")
            .arg(&k)
            .arg(self.get_ttl(ttl))
            .arg("NX") // 2. 只有它没有 TTL 时（即刚创建时）才设 TTL
            .query_async::<(i64, bool)>(&mut conn)
            .await
            .context(RedisSnafu { category: "incr" })?;
        Ok(count)
    }

    /// 向 Redis 写入值，TTL 为 None 时使用实例默认值。
    pub async fn set<T: redis::ToSingleRedisArg + Send + Sync>(
        &self,
        key: &str,
        value: T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        self.set_value(&self.get_key(key), value, self.get_ttl(ttl))
            .await
    }

    /// 从 Redis 读取值，类型由泛型参数指定。
    pub async fn get<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        self.get_value::<T>(&self.get_key(key)).await
    }

    /// 将结构体序列化为 JSON 后存入 Redis。
    pub async fn set_struct<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).context(SerdeJsonSnafu)?;
        self.set_value(&self.get_key(key), &value, self.get_ttl(ttl))
            .await?;
        Ok(())
    }

    /// 从 Redis 读取并反序列化为结构体，键不存在时返回 `None`。
    pub async fn get_struct<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let buf: Option<Vec<u8>> = self.get_value(&self.get_key(key)).await?;
        match buf {
            None => Ok(None),
            Some(b) => serde_json::from_slice(&b).context(SerdeJsonSnafu).map(Some),
        }
    }

    /// 获取指定键的剩余过期时间（秒）。
    /// 返回 -2 表示键不存在，-1 表示键无过期时间。
    pub async fn ttl(&self, key: &str) -> Result<i32> {
        let result = self
            .conn()
            .await?
            .ttl(self.get_key(key))
            .await
            .context(RedisSnafu { category: "ttl" })?;

        Ok(result)
    }

    /// 原子性地读取并删除指定键（需 Redis ≥6.2.0）。
    pub async fn get_del<T: redis::FromRedisValue>(&self, key: &str) -> Result<T> {
        let result = self
            .conn()
            .await?
            .get_del(self.get_key(key))
            .await
            .context(RedisSnafu {
                category: "get_del",
            })?;

        Ok(result)
    }

    /// 检查指定键是否存在。
    pub async fn exists(&self, key: &str) -> Result<bool> {
        let result = self
            .conn()
            .await?
            .exists(self.get_key(key))
            .await
            .context(RedisSnafu { category: "exists" })?;
        Ok(result)
    }

    /// 刷新指定键的过期时间而不修改其值。
    /// 返回 `true` 表示刷新成功，`false` 表示键不存在。
    pub async fn expire(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let result = self
            .conn()
            .await?
            .expire(self.get_key(key), self.get_ttl(ttl) as i64)
            .await
            .context(RedisSnafu { category: "expire" })?;
        Ok(result)
    }

    async fn set_struct_compressed<T>(
        &self,
        key: &str,
        value: &T,
        ttl: u64,
        algorithm: Algorithm,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(value).context(SerdeJsonSnafu)?;
        let buf = compress(&value, algorithm).context(CompressionSnafu)?;
        self.set_value(key, &buf, ttl).await
    }

    async fn get_struct_compressed<T>(&self, key: &str, algorithm: Algorithm) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let value: Option<Vec<u8>> = self.get_value(&self.get_key(key)).await?;
        match value {
            None => Ok(None),
            Some(compressed_buf) => {
                let buf = decompress(&compressed_buf, algorithm).context(CompressionSnafu)?;
                serde_json::from_slice(&buf)
                    .context(SerdeJsonSnafu)
                    .map(Some)
            }
        }
    }

    /// 将结构体序列化为 JSON 并以 LZ4 压缩后存入 Redis。
    /// LZ4 压缩速度快，适合对延迟敏感的场景。
    pub async fn set_struct_lz4<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        self.set_struct_compressed(&self.get_key(key), value, self.get_ttl(ttl), Algorithm::Lz4)
            .await
    }

    /// 从 Redis 读取并以 LZ4 解压后反序列化为结构体，键不存在时返回 `None`。
    pub async fn get_struct_lz4<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.get_struct_compressed(key, Algorithm::Lz4).await
    }

    /// 将结构体序列化为 JSON 并以 Zstd 压缩后存入 Redis。
    /// Zstd 压缩率更高，适合对存储空间敏感的场景。
    pub async fn set_struct_zstd<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        self.set_struct_compressed(&self.get_key(key), value, self.get_ttl(ttl), DEFAULT_ZSTD)
            .await
    }

    /// 从 Redis 读取并以 Zstd 解压后反序列化为结构体，键不存在时返回 `None`。
    pub async fn get_struct_zstd<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.get_struct_compressed(key, DEFAULT_ZSTD).await
    }
}
