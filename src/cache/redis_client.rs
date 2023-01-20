use once_cell::sync::OnceCell;
use r2d2::{Pool, PooledConnection};
use redis::{Client, Commands};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu, Whatever};
use std::{ops::DerefMut, slice::from_raw_parts, time::Duration};

use crate::{
    config::must_new_redis_config,
    error::HTTPError,
    util::{snappy_decode, snappy_encode, zstd_decode, zstd_encode},
};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Redis fail, category {}: {}", category, source))]
    Redis {
        category: String,
        source: redis::RedisError,
    },
    #[snafu(display("Redis r2d2 fail, category {}: {}", category, source))]
    R2d2 {
        category: String,
        source: r2d2::Error,
    },
    #[snafu(display("Json fail: {}", source))]
    Json { source: serde_json::Error },
    #[snafu(display("{}", source))]
    Whatever { source: Whatever },
}
pub type Result<T, E = Error> = std::result::Result<T, E>;
// 自动转换为http error
impl From<Error> for HTTPError {
    fn from(err: Error) -> Self {
        // TODO 是否基于不同的error来转换
        HTTPError::new_with_category(err.to_string().as_str(), "redis")
    }
}
impl From<Whatever> for Error {
    fn from(err: Whatever) -> Self {
        Error::Whatever { source: err }
    }
}
impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json { source: err }
    }
}

static REDIS_POOL: OnceCell<Pool<Client>> = OnceCell::new();

pub fn must_new_redis_client() -> Client {
    let config = must_new_redis_config();
    Client::open(config.uri).unwrap()
}

fn get_redis_pool() -> Result<Pool<Client>> {
    let result = REDIS_POOL.get_or_try_init(|| -> Result<Pool<Client>> {
        // must new redis client 已成功
        // 因此获取配置不会再失败
        let config = must_new_redis_config();
        let client = Client::open(config.uri).context(RedisSnafu {
            category: "clientOpen".to_string(),
        })?;
        // TODO 实现async的r2d2
        let pool = r2d2::Pool::builder()
            .max_size(config.pool_size)
            .min_idle(Some(config.idle))
            .connection_timeout(config.connection_timeout)
            .build(client)
            .context(R2d2Snafu {
                category: "r2d2Build",
            })?;
        // TODO 添加error_handler event_handler
        Ok(pool)
    })?;

    Ok(result.clone())
}

pub struct RedisCache {
    pool: Pool<Client>,
    ttl: Duration,
}
impl RedisCache {
    fn get_conn(&self) -> Result<PooledConnection<Client>> {
        self.pool.get().context(R2d2Snafu {
            category: "poolGet",
        })
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
    pub fn lock(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let mut conn = self.get_conn()?;
        let result = redis::cmd("SET")
            .arg(key)
            .arg(true)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .query(conn.deref_mut())
            .context(RedisSnafu { category: "lock" })?;
        Ok(result)
    }
    /// Del a key from cache
    pub fn del(&self, key: &str) -> Result<()> {
        let mut conn = self.get_conn()?;
        conn.del(key).context(RedisSnafu { category: "del" })?;
        Ok(())
    }
    /// Increase the value of key, if ttl is none,
    /// the default ttl will be used.
    pub fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        let mut conn = self.get_conn()?;
        let (_, count) = redis::pipe()
            .cmd("SET")
            .arg(key)
            .arg(0)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .cmd("INCRBY")
            .arg(key)
            .arg(delta)
            .query::<(bool, i64)>(conn.deref_mut())
            .context(RedisSnafu { category: "incr" })?;
        Ok(count)
    }
    /// Set bytes value to cache with ttl, if ttl is none,
    /// the default ttl will be used.
    pub fn set_bytes(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let mut conn = self.get_conn()?;
        let seconds = ttl.unwrap_or(self.ttl).as_secs();
        conn.set_ex(key, value, seconds as usize)
            .context(RedisSnafu {
                category: "setBytes",
            })?;
        Ok(())
    }
    /// Get bytes value from cache
    pub fn get_bytes(&self, key: &str) -> Result<Vec<u8>> {
        let mut conn = self.get_conn()?;
        let result = conn.get(key).context(RedisSnafu {
            category: "getBytes",
        })?;
        Ok(result)
    }
    /// Set struct to cache with ttl, if ttl is none,
    /// the default ttl will be used.
    pub fn set_struct<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value)?;
        self.set_bytes(key, value, ttl)?;
        Ok(())
    }
    /// Get struct from cache
    pub fn get_struct<'a, T>(&self, key: &str) -> Result<T>
    where
        T: Default + Deserialize<'a>,
    {
        let value = self.get_bytes(key)?;

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
    pub fn ttl(&self, key: &str) -> Result<i32> {
        let mut conn = self.get_conn()?;
        let result = conn.ttl(key).context(RedisSnafu { category: "ttl" })?;
        Ok(result)
    }
    // GetDel gets the value of key and delete it
    pub fn get_del(&self, key: &str) -> Result<Vec<u8>> {
        let mut conn = self.get_conn()?;
        let (value, _) = redis::pipe()
            .cmd("GET")
            .arg(key)
            .cmd("DEL")
            .arg(key)
            .query::<(Vec<u8>, bool)>(conn.deref_mut())
            .context(RedisSnafu { category: "getDel" })?;
        Ok(value)
    }
    // Set struct to cache, the data will be compressed using snappy
    pub fn set_struct_snappy<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value)?;
        let buf = snappy_encode(&value)?;
        self.set_bytes(key, buf, ttl)?;
        Ok(())
    }
    // Get struct from cache, the data will be decompressed using snappy
    pub fn get_struct_snappy<'a, T>(&self, key: &str) -> Result<T>
    where
        T: Default + Deserialize<'a>,
    {
        let value = self.get_bytes(key)?;

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
    pub fn set_struct_zstd<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).context(JsonSnafu {})?;
        let buf = zstd_encode(&value)?;
        self.set_bytes(key, buf, ttl)?;
        Ok(())
    }
    // Get struct from cache, the data will be decompressed using zstd
    pub fn get_struct_zstd<'a, T>(&self, key: &str) -> Result<T>
    where
        T: Default + Deserialize<'a>,
    {
        let value = self.get_bytes(key)?;

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
