// Copyright 2026 Tree xie.
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

use super::{
    CompressionSnafu, Error, RedisClient, RedisClientConn, RedisDedicatedConn, RedisSnafu,
    SerdeJsonSnafu,
};
use redis::{AsyncCommands, cmd, pipe};
use serde::{Serialize, de::DeserializeOwned};
use snafu::ResultExt;
use std::{borrow::Cow, time::Duration};
use tibba_util::{Algorithm, compress, decompress};

const DEFAULT_ZSTD: Algorithm = Algorithm::Zstd(3);

type Result<T> = std::result::Result<T, Error>;

/// `Duration` → Redis 毫秒过期时长，下限 1、上限饱和。
///
/// 抽成自由函数只为可测：`RedisCache` 需要一个 `&'static RedisClient` 才能构造，
/// 而这段纯算术是最容易出错、也最该被钉住的部分。
#[inline]
fn ttl_to_millis(ttl: Duration) -> u64 {
    u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX).max(1)
}

/// Redis 缓存封装，提供键值读写、分布式锁、计数器等常用缓存操作。
///
/// `Clone` 很廉价：只有一个 `Duration`、一个前缀 `String` 和一个 `&'static`
/// 连接池引用，不复制任何连接。需要「同一个池、不同 TTL / 前缀」的视图时
/// （例如把它交给 [`crate::TwoLevelStore`]）克隆一份再链式改写即可。
#[derive(Clone)]
pub struct RedisCache {
    /// 缓存条目的默认过期时长
    ttl: Duration,
    /// 所有缓存键统一添加的前缀
    prefix: String,
    /// Redis 连接池
    client: &'static RedisClient,
}

impl RedisCache {
    /// 从连接池借用连接（短命令）。长阻塞请用 [`Self::dedicated_blocking_conn`]。
    #[inline]
    pub async fn conn(&self) -> Result<RedisClientConn> {
        self.client.conn().await
    }

    /// 阻塞读专用连接。`max_block` 与 `BRPOP` timeout 对齐，见 [`RedisClient::dedicated_blocking_conn`]。
    #[inline]
    pub async fn dedicated_blocking_conn(&self, max_block: Duration) -> Result<RedisDedicatedConn> {
        self.client.dedicated_blocking_conn(max_block).await
    }

    /// 短命令专用写连接（reply_loop），见 [`RedisClient::dedicated_command_conn`]。
    #[inline]
    pub async fn dedicated_command_conn(&self) -> Result<RedisDedicatedConn> {
        self.client.dedicated_command_conn().await
    }

    /// 底层 [`RedisClient`] 引用。
    #[inline]
    pub fn client(&self) -> &'static RedisClient {
        self.client
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

    /// 解析出本次操作要用的 TTL，单位**毫秒**，下限钳为 1。
    ///
    /// # 为什么是毫秒
    /// 此前是 `as_secs()`：任何亚秒 TTL 都会被截断成 0，随后 `SET ... EX 0` /
    /// `EXPIRE ... 0` 被 Redis 直接拒绝（`invalid expire time`）。也就是说
    /// `with_ttl(Duration::from_millis(500))` 不是「按 500ms 过期」，而是
    /// **运行期报错**——而限流这类场景恰恰会想要亚秒窗口。
    ///
    /// 改用 Redis 原生的毫秒指令族（`PSETEX` / `PX` / `PEXPIRE`）后，亚秒 TTL
    /// 按真实值生效。下限 1ms 只为挡住 `Duration::ZERO`（同样会被 Redis 拒绝）。
    #[inline]
    pub(crate) fn get_ttl_ms(&self, ttl: Option<Duration>) -> u64 {
        ttl_to_millis(ttl.unwrap_or(self.ttl))
    }

    /// 拼接前缀与键名，生成完整的缓存键。
    /// 前缀为空时直接借用原始键，避免额外分配。
    #[inline]
    pub(crate) fn get_key<'a>(&'a self, key: &'a str) -> Cow<'a, str> {
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

    /// 向 Redis 写入原始值，并设置过期时间（毫秒）。
    async fn set_value<T: redis::ToSingleRedisArg + Send + Sync>(
        &self,
        key: &str,
        value: T,
        ttl_ms: u64,
    ) -> Result<()> {
        let () = self
            .conn()
            .await?
            .pset_ex(key, value, ttl_ms)
            .await
            .context(RedisSnafu { category: "set" })?;
        Ok(())
    }

    /// 尝试通过 `SET NX EX` 获取分布式锁。
    /// 返回 `true` 表示加锁成功，`false` 表示锁已被持有。
    ///
    /// # 语义：只靠 TTL 释放，刻意不提供 unlock
    /// 锁值固定为 `true`，不携带 owner token，也**没有配套的 `unlock()`**——这是设计
    /// 选择而非遗漏。本锁的用途是「同一时间窗内全集群只跑一次」（定时任务去重、
    /// 探测任务抢占），而不是互斥临界区：
    ///
    /// - **提前释放反而有害**：任务跑完就解锁，会让同一触发窗口内的其它实例立刻
    ///   抢到锁重跑一遍，正好破坏「只跑一次」的目的。锁持有到 TTL 自然过期，才等于
    ///   一个干净的去重窗口。
    /// - **无 owner 校验是安全的**：既然没人主动释放，就不存在「误删他人锁」的路径。
    ///   若将来要加 `unlock()`，必须同时引入随机 token + Lua CAS 释放，否则在
    ///   「锁已过期并被另一实例重新持有」的窗口里会删掉别人的锁。
    ///
    /// # 调用方约束
    /// - `ttl` 应 **≥ 任务预期执行时长**，且 **≤ 触发间隔**（取值权衡见
    ///   `tibba_runtime::singleton_cron_job`）。
    /// - 任务体仍须**幂等**：执行时长超过 TTL 的极端情况下，仍可能出现并发执行。
    pub async fn lock(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let mut conn = self.conn().await?;

        let result = cmd("SET")
            .arg(self.get_key(key))
            .arg(true)
            .arg("NX")
            // PX 而非 EX：亚秒锁期在限流 / 高频去重里是合理需求，用秒会被截断成 0
            .arg("PX")
            .arg(self.get_ttl_ms(ttl))
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
    /// INCRBY 在键不存在时自动创建（初值 0），随后 `PEXPIRE ... NX` 仅在键尚无 TTL
    /// （即刚创建）时设置过期，避免每次累加都刷新 TTL。用 pipeline 保证两条命令原子执行。
    ///
    /// 用 `PEXPIRE` 而非 `EXPIRE`：固定窗口限流常要亚秒窗口，秒级会被截断成 0
    /// 并让整条命令失败。
    pub async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        let mut conn = self.conn().await?;
        let k = self.get_key(key);
        let (count, _) = pipe()
            .cmd("INCRBY")
            .arg(&k)
            .arg(delta) // 1. 累加（键不存在自动创建，初值 0，此时无 TTL）
            .cmd("PEXPIRE")
            .arg(&k)
            .arg(self.get_ttl_ms(ttl))
            .arg("NX") // 2. 仅当键尚无 TTL（刚创建）时才设，避免每次累加刷新过期
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
        self.set_value(&self.get_key(key), value, self.get_ttl_ms(ttl))
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
        self.set_value(&self.get_key(key), &value, self.get_ttl_ms(ttl))
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
    pub async fn ttl(&self, key: &str) -> Result<i64> {
        let result = self
            .conn()
            .await?
            .ttl(self.get_key(key))
            .await
            .context(RedisSnafu { category: "ttl" })?;

        Ok(result)
    }

    /// 一次往返批量读取多个键（MGET），结果与 `keys` 顺序一一对应，不存在的键为 `None`。
    /// `keys` 为空时直接返回空集合，不访问 Redis。
    pub async fn mget<T: redis::FromRedisValue>(&self, keys: &[String]) -> Result<Vec<Option<T>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let mut command = cmd("MGET");
        for key in keys {
            command.arg(self.get_key(key).as_ref());
        }
        let result = command
            .query_async(&mut self.conn().await?)
            .await
            .context(RedisSnafu { category: "mget" })?;
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
        let ms = i64::try_from(self.get_ttl_ms(ttl)).unwrap_or(i64::MAX);
        let result = self
            .conn()
            .await?
            .pexpire(self.get_key(key), ms)
            .await
            .context(RedisSnafu { category: "expire" })?;
        Ok(result)
    }

    /// 获取指定键的剩余过期时间（毫秒），精度高于 [`Self::ttl`]。
    /// 返回 -2 表示键不存在，-1 表示键无过期时间。
    pub async fn ttl_ms(&self, key: &str) -> Result<i64> {
        let result = self
            .conn()
            .await?
            .pttl(self.get_key(key))
            .await
            .context(RedisSnafu { category: "pttl" })?;
        Ok(result)
    }

    async fn set_struct_compressed<T>(
        &self,
        key: &str,
        value: &T,
        ttl_ms: u64,
        algorithm: Algorithm,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(value).context(SerdeJsonSnafu)?;
        let buf = compress(&value, algorithm).context(CompressionSnafu)?;
        self.set_value(key, &buf, ttl_ms).await
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
        self.set_struct_compressed(
            &self.get_key(key),
            value,
            self.get_ttl_ms(ttl),
            Algorithm::Lz4,
        )
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
        self.set_struct_compressed(
            &self.get_key(key),
            value,
            self.get_ttl_ms(ttl),
            DEFAULT_ZSTD,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// **回归守卫**：亚秒 TTL 必须按真实值下发，而不是被截断成 0。
    ///
    /// 旧实现是 `as_secs()`：`500ms` → `0` → `SET ... EX 0`，Redis 直接回
    /// `invalid expire time`。也就是说 `with_ttl(500ms)` 不是「按 500ms 过期」，
    /// 而是让**每一次写入**在运行期失败。
    #[test]
    fn sub_second_ttl_is_preserved() {
        assert_eq!(ttl_to_millis(Duration::from_millis(500)), 500);
        assert_eq!(ttl_to_millis(Duration::from_millis(1)), 1);
        assert_eq!(
            ttl_to_millis(Duration::from_micros(100)),
            1,
            "不足 1ms 取下限"
        );
    }

    /// 零时长同样会被 Redis 拒绝，钳到 1ms。
    #[test]
    fn zero_ttl_is_clamped_to_one_millisecond() {
        assert_eq!(ttl_to_millis(Duration::ZERO), 1);
    }

    #[test]
    fn whole_seconds_convert_exactly() {
        assert_eq!(ttl_to_millis(Duration::from_secs(1)), 1_000);
        assert_eq!(ttl_to_millis(Duration::from_secs(600)), 600_000);
    }

    /// 超长时长饱和而非回绕（回绕会得到一个极短 TTL，缓存瞬间失效）。
    #[test]
    fn absurd_ttl_saturates_instead_of_wrapping() {
        assert_eq!(ttl_to_millis(Duration::MAX), u64::MAX);
    }
}
