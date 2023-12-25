use crate::config::must_new_redis_config;
use deadpool_redis::cluster::Pool;
use deadpool_redis::cluster::{Config, Runtime};
use deadpool_redis::PoolConfig;
use once_cell::sync::OnceCell;

pub fn must_get_redis_pool() -> &'static Pool {
    static REDIS_POOL: OnceCell<Pool> = OnceCell::new();
    REDIS_POOL
        .get_or_try_init(|| {
            let config = must_new_redis_config();
            let mut cfg = Config::from_urls(config.nodes);
            cfg.pool = Some(PoolConfig {
                max_size: config.pool_size as usize,
                timeouts: deadpool_redis::Timeouts {
                    wait: Some(config.wait_timeout),
                    create: Some(config.connection_timeout),
                    recycle: Some(config.recycle_timeout),
                },
                ..Default::default()
            });
            cfg.create_pool(Some(Runtime::Tokio1))
        })
        .unwrap()
}
