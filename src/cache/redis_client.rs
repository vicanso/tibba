use crate::{
    config::must_new_redis_config,
    error::HTTPError,
    util::{snappy_decode, snappy_encode, zstd_decode, zstd_encode},
};
use redis::Client;

use deadpool_redis::{
    redis::{cmd, pipe},
    Connection, Manager, Pool, PoolConfig, Runtime,
};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu, Whatever};
use std::{slice::from_raw_parts, time::Duration};

pub fn must_new_redis_client() -> Client {
    let config = must_new_redis_config();
    Client::open(config.uri).unwrap()
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Redis fail, category:{}, {}", category, source))]
    Redis {
        category: String,
        source: deadpool_redis::redis::RedisError,
    },
    #[snafu(display("Create redis pool fail, {}", source))]
    CreatePool { source: deadpool_redis::BuildError },
    #[snafu(display("Redis pool fail, {}", source))]
    Pool { source: deadpool_redis::PoolError },
    #[snafu(display("Json fail, {}", source))]
    Json { source: serde_json::Error },
    #[snafu(display("{}", source))]
    Whatever { source: Whatever },
}
impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json { source: err }
    }
}
impl From<Whatever> for Error {
    fn from(err: Whatever) -> Self {
        Error::Whatever { source: err }
    }
}
impl From<Error> for HTTPError {
    fn from(err: Error) -> Self {
        HTTPError::new_with_category(err.to_string().as_str(), "redisClient")
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

fn get_redis_pool() -> Result<&'static Pool> {
    static REDIS_POOL: OnceCell<Pool> = OnceCell::new();
    let result = REDIS_POOL.get_or_try_init(|| -> Result<Pool> {
        let config = must_new_redis_config();
        let p = Pool::builder(Manager::new(config.uri.as_str()).unwrap());
        let pool = p
            .config(PoolConfig {
                max_size: config.pool_size as usize,
                timeouts: deadpool_redis::Timeouts {
                    wait: Some(config.wait_timeout),
                    create: Some(config.connection_timeout),
                    recycle: Some(config.recycle_timeout),
                },
            })
            .runtime(Runtime::Tokio1)
            .build()
            .context(CreatePoolSnafu {})?;
        Ok(pool)
    })?;
    Ok(result)
}

pub struct RedisCache {
    pool: &'static Pool,
    ttl: Duration,
}

/// 获取默认的redis缓存，基于redis pool并设置默认的ttl
pub async fn get_default_redis_cache() -> Result<&'static RedisCache> {
    static DEFAULT_REDIS_CACHE: OnceCell<RedisCache> = OnceCell::new();

    DEFAULT_REDIS_CACHE.get_or_try_init(|| -> Result<RedisCache> { RedisCache::new() })
}

impl RedisCache {
    async fn get_conn(&self) -> Result<Connection> {
        self.pool.get().await.context(PoolSnafu {})
    }
    /// 使用默认的ttl初始化redis缓存实例
    pub fn new() -> Result<RedisCache> {
        Self::new_with_ttl(Duration::from_secs(5 * 60))
    }
    /// 初始化redis缓存实例，并指定默认的ttl
    pub fn new_with_ttl(ttl: Duration) -> Result<RedisCache> {
        let pool = get_redis_pool()?;
        Ok(RedisCache { pool, ttl })
    }
    /// 尝试锁定key，时长为ttl，若未指定时长则使用默认时长
    /// 如果成功则返回true，否则返回false。
    /// 主要用于多实例并发限制。
    pub async fn lock(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let mut conn = self.get_conn().await?;

        let result = cmd("SET")
            .arg(key)
            .arg(true)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "lock" })?;
        Ok(result)
    }
    /// 从redis中删除key
    pub async fn del(&self, key: &str) -> Result<()> {
        let mut conn = self.get_conn().await?;

        cmd("DEL")
            .arg(key)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "del" })?;
        Ok(())
    }
    /// 增加redis中key所对应的值，如果ttl未指定则使用默认值，
    /// 需要注意此ttl仅在首次时设置。
    pub async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        let mut conn = self.get_conn().await?;
        let (_, count) = pipe()
            .cmd("SET")
            .arg(key)
            .arg(0)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .cmd("INCRBY")
            .arg(key)
            .arg(delta)
            .query_async::<Connection, (bool, i64)>(&mut conn)
            .await
            .context(RedisSnafu { category: "incr" })?;
        Ok(count)
    }
    /// 将数据设置至redis中，如果未设置ttl则使用默认值
    pub async fn set_bytes(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let mut conn = self.get_conn().await?;

        let seconds = ttl.unwrap_or(self.ttl).as_secs();
        cmd("SETEX")
            .arg(key)
            .arg(seconds)
            .arg(value)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu {
                category: "setBytes",
            })?;
        Ok(())
    }
    /// 从redis中获取数据
    pub async fn get_bytes(&self, key: &str) -> Result<Vec<u8>> {
        let mut conn = self.get_conn().await?;
        let result = cmd("GET")
            .arg(key)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu {
                category: "getBytes",
            })?;

        Ok(result)
    }
    /// 将struct转换为json后设置至redis中，若未指定ttl则使用默认值
    pub async fn set_struct<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value)?;
        self.set_bytes(key, value, ttl).await?;
        Ok(())
    }
    /// 从redis中获取数据并转换为struct，如果缓存中无数据则使用struct的默认值返回
    pub async fn get_struct<'a, T>(&self, key: &str) -> Result<T>
    where
        T: Default + Deserialize<'a>,
    {
        let value = self.get_bytes(key).await?;

        if value.is_empty() {
            return Ok(T::default());
        }

        // TODO 生命周期是否有其它方法调整
        let result = unsafe {
            let p = value.as_ptr();
            serde_json::from_slice(from_raw_parts(p, value.len()))?
        };

        Ok(result)
    }
    /// 返回该key在redis中的有效期
    pub async fn ttl(&self, key: &str) -> Result<i32> {
        let mut conn = self.get_conn().await?;
        let result = cmd("TTL")
            .arg(key)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "ttl" })?;
        Ok(result)
    }
    /// 获取后并删除该key在redis中的值，用于仅获取一次的场景
    pub async fn get_del(&self, key: &str) -> Result<Vec<u8>> {
        let mut conn = self.get_conn().await?;
        let (value, _) = pipe()
            .cmd("GET")
            .arg(key)
            .cmd("DEL")
            .arg(key)
            .query_async::<Connection, (Vec<u8>, bool)>(&mut conn)
            .await
            .context(RedisSnafu { category: "getDel" })?;
        Ok(value)
    }
    /// 将struct转换为json后使用snappy压缩，
    /// 再将压缩后的数据设置至redis中，若未指定ttl，
    /// 则使用默认的有效期
    pub async fn set_struct_snappy<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value)?;
        let buf = snappy_encode(&value)?;
        self.set_bytes(key, buf, ttl).await?;
        Ok(())
    }
    /// 从redis获取数据后使用snappy解压，并转换为对应的struct
    pub async fn get_struct_snappy<'a, T>(&self, key: &str) -> Result<T>
    where
        T: Default + Deserialize<'a>,
    {
        let value = self.get_bytes(key).await?;

        if value.is_empty() {
            return Ok(T::default());
        }

        let buf = snappy_decode(value.as_slice())?;

        // TODO 生命周期是否有其它方法调整
        let result = unsafe {
            let p = buf.as_ptr();
            serde_json::from_slice(from_raw_parts(p, buf.len()))?
        };

        Ok(result)
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
        let value = serde_json::to_vec(&value).context(JsonSnafu {})?;
        let buf = zstd_encode(&value)?;
        self.set_bytes(key, buf, ttl).await?;
        Ok(())
    }
    /// 从redis获取数据后使用zstd解压，并转换为对应的struct
    pub async fn get_struct_zstd<'a, T>(&self, key: &str) -> Result<T>
    where
        T: Default + Deserialize<'a>,
    {
        let value = self.get_bytes(key).await?;

        if value.is_empty() {
            return Ok(T::default());
        }

        let buf = zstd_decode(value.as_slice())?;

        // TODO 生命周期是否有其它方法调整
        let result = unsafe {
            let p = buf.as_ptr();
            serde_json::from_slice(from_raw_parts(p, buf.len()))?
        };

        Ok(result)
    }
}
