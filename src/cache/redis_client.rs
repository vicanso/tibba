use once_cell::sync::OnceCell;
use r2d2::Pool;
use redis::{Client, Commands};
use serde::{Deserialize, Serialize};
use snap::{read::FrameDecoder, write::FrameEncoder};
use std::{ops::DerefMut, slice::from_raw_parts, time::Duration};

use crate::{
    config::must_new_redis_config,
    error::{HTTPError, HTTPResult},
};

static REDIS_POOL: OnceCell<Pool<Client>> = OnceCell::new();

pub fn must_new_redis_client() -> Client {
    let config = must_new_redis_config();
    Client::open(config.uri).unwrap()
}

fn get_redis_pool() -> HTTPResult<Pool<Client>> {
    let result = REDIS_POOL.get_or_try_init(|| -> HTTPResult<Pool<Client>> {
        // must new redis client 已成功
        // 因此获取配置不会再失败
        let config = must_new_redis_config();
        let client = Client::open(config.uri)?;
        let pool = r2d2::Pool::builder()
            .max_size(config.pool_size)
            .min_idle(Some(config.idle))
            .connection_timeout(config.connection_timeout)
            .build(client)?;
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
    pub fn new() -> HTTPResult<RedisCache> {
        let pool = get_redis_pool()?;
        Ok(RedisCache {
            pool,
            ttl: Duration::from_secs(5 * 60),
        })
    }
    /// Lock a key with ttl, if ttl is none,
    /// the default ttl will be used.
    pub fn lock(&self, key: &str, ttl: Option<Duration>) -> HTTPResult<bool> {
        let mut conn = self.pool.get()?;
        let result = redis::cmd("SET")
            .arg(key)
            .arg(true)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .query(conn.deref_mut())?;
        Ok(result)
    }
    /// Del a key from cache
    pub fn del(&self, key: &str) -> HTTPResult<()> {
        let mut conn = self.pool.get()?;
        conn.del(key)?;
        Ok(())
    }
    /// Increase the value of key, if ttl is none,
    /// the default ttl will be used.
    pub fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> HTTPResult<i64> {
        let mut conn = self.pool.get()?;
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
            .query::<(bool, i64)>(conn.deref_mut())?;
        Ok(count)
    }
    /// Set bytes value to cache with ttl, if ttl is none,
    /// the default ttl will be used.
    pub fn set_bytes(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> HTTPResult<()> {
        let mut conn = self.pool.get()?;
        let seconds = ttl.unwrap_or(self.ttl).as_secs();
        conn.set_ex(key, value, seconds as usize)?;
        Ok(())
    }
    /// Get bytes value from cache
    pub fn get_bytes(&self, key: &str) -> HTTPResult<Vec<u8>> {
        let mut conn = self.pool.get()?;
        let result = conn.get(key)?;
        Ok(result)
    }
    /// Set struct to cache with ttl, if ttl is none,
    /// the default ttl will be used.
    pub fn set_struct<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> HTTPResult<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value)?;
        self.set_bytes(key, value, ttl)?;
        Ok(())
    }
    /// Get struct from cache
    pub fn get_struct<'a, T>(&self, key: &str) -> HTTPResult<T>
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
    pub fn ttl(&self, key: &str) -> HTTPResult<i32> {
        let mut conn = self.pool.get()?;
        let result = conn.ttl(key)?;
        Ok(result)
    }
    // GetDel gets the value of key and delete it
    pub fn get_del(&self, key: &str) -> HTTPResult<Vec<u8>> {
        let mut conn = self.pool.get()?;
        let (value, _) = redis::pipe()
            .cmd("GET")
            .arg(key)
            .cmd("DEL")
            .arg(key)
            .query::<(Vec<u8>, bool)>(conn.deref_mut())?;
        Ok(value)
    }
    // Set struct to cache, the data will be compressed using snappy
    pub fn set_struct_snappy<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> HTTPResult<()>
    where
        T: ?Sized + Serialize,
    {
        use std::io::Write;
        let value = serde_json::to_vec(&value)?;
        let mut writer = FrameEncoder::new(vec![]);
        writer.write_all(&value)?;
        let data = writer
            .into_inner()
            .map_err(|err| HTTPError::new(err.to_string().as_str()))?;

        self.set_bytes(key, data, ttl)?;
        Ok(())
    }
    // Get struct from cache, the data will be decompressed using snappy
    pub fn get_struct_snappy<'a, T>(&self, key: &str) -> HTTPResult<T>
    where
        T: Default + Deserialize<'a>,
    {
        use std::io::Read;
        let value = self.get_bytes(key)?;

        if value.is_empty() {
            return Ok(T::default());
        }

        let mut buf = vec![];
        FrameDecoder::new(value.as_slice()).read_to_end(&mut buf)?;

        // TODO 生命周期是否有其它方法调整
        let result = unsafe {
            let p = buf.as_ptr();
            serde_json::from_slice(from_raw_parts(p, buf.len()))?
        };

        Ok(result)
    }
}
