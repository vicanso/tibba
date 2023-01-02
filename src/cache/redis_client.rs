use once_cell::sync::OnceCell;
use r2d2::Pool;
use redis::{Client, ConnectionLike};
use std::time::Duration;

use crate::{config::must_new_redis_config, error::HTTPResult};

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
    /// Lock a key with ttl, if ttl is none, the default ttl will be used.
    pub fn lock(&self, key: String, ttl: Option<Duration>) -> HTTPResult<bool> {
        let mut conn = self.pool.get()?;
        let result = conn.req_command(
            redis::cmd("SET")
                .arg(key)
                .arg(true)
                .arg("NX")
                .arg("EX")
                .arg(ttl.unwrap_or(self.ttl).as_secs()),
        )?;
        Ok(result == redis::Value::Okay)
    }
}
