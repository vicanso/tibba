use once_cell::sync::OnceCell;
use r2d2::Pool;
use redis::Client;

use crate::{config::must_new_redis_config, error::HTTPError};

static REDIS_POOL: OnceCell<Pool<Client>> = OnceCell::new();

pub fn must_new_redis_client() -> Client {
    let config = must_new_redis_config();
    Client::open(config.uri).unwrap()
}

pub fn get_redis_pool() -> Result<&'static Pool<Client>, HTTPError> {
    REDIS_POOL.get_or_try_init(|| -> Result<Pool<Client>, HTTPError> {
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
    })
}
