use crate::{
    config::must_new_redis_config,
    error::HTTPError,
    util::{snappy_decode, snappy_encode, zstd_decode, zstd_encode},
};
use redis::Client;

use deadpool_redis::{
    redis::{cmd, pipe, FromRedisValue},
    Config, Connection, Pool, Runtime,
};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu, Whatever};
use std::{ops::DerefMut, slice::from_raw_parts, time::Duration};

pub fn must_new_redis_client() -> Client {
    let config = must_new_redis_config();
    Client::open(config.uri).unwrap()
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Redis fail, category {}: {}", category, source))]
    Redis {
        category: String,
        source: deadpool_redis::redis::RedisError,
    },
    #[snafu(display("Create redis pool fail, {}", source))]
    CreatePool {
        source: deadpool_redis::CreatePoolError,
    },
    #[snafu(display("Redis pool fail, {}", source))]
    Pool { source: deadpool_redis::PoolError },
    #[snafu(display("Json fail: {}", source))]
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

pub type Result<T, E = Error> = std::result::Result<T, E>;

fn get_redis_pool() -> Result<&'static Pool> {
    static REDIS_POOL: OnceCell<Pool> = OnceCell::new();
    let result = REDIS_POOL.get_or_try_init(|| -> Result<Pool> {
        let config = must_new_redis_config();
        let pool = Config::from_url(config.uri)
            .create_pool(Some(Runtime::Tokio1))
            .context(CreatePoolSnafu {})?;
        Ok(pool)
    })?;
    Ok(result)
}

pub struct RedisCache {
    pool: &'static Pool,
    ttl: Duration,
}

impl RedisCache {
    async fn get_conn(&self) -> Result<Connection> {
        self.pool.get().await.context(PoolSnafu {})
    }
    pub fn new() -> Result<RedisCache> {
        let pool = get_redis_pool()?;
        Ok(RedisCache {
            pool,
            ttl: Duration::from_secs(5 * 60),
        })
    }
    /// Lock a key with ttl, if ttl is none,
    /// the default ttl will be used.
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
    /// Del a key from cache
    pub async fn del(&self, key: &str) -> Result<()> {
        let mut conn = self.get_conn().await?;

        cmd("DEL")
            .arg(key)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "del" })?;
        Ok(())
    }
    /// Increase the value of key, if ttl is none,
    /// the default ttl will be used.
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
    /// Set bytes value to cache with ttl, if ttl is none,
    /// the default ttl will be used.
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
    /// Get bytes value from cache
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
    /// Set struct to cache with ttl, if ttl is none,
    /// the default ttl will be used.
    pub async fn set_struct<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value)?;
        self.set_bytes(key, value, ttl).await?;
        Ok(())
    }
    /// Get struct from cache
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
    /// Ttl returns the ttl of key
    pub async fn ttl(&self, key: &str) -> Result<i32> {
        let mut conn = self.get_conn().await?;
        let result = cmd("TTL")
            .arg(key)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "ttl" })?;
        Ok(result)
    }
    // GetDel gets the value of key and delete it
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
    // Set struct to cache, the data will be compressed using snappy
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
    // Get struct from cache, the data will be decompressed using snappy
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
    // Set struct to cache, the data will be compressed using zstd
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
    // Get struct from cache, the data will be decompressed using zstd
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
