use crate::config::must_new_redis_config;
use deadpool_redis::{Manager, Pool, PoolConfig, Runtime};
use once_cell::sync::OnceCell;

pub fn must_get_redis_pool() -> &'static Pool {
    static REDIS_POOL: OnceCell<Pool> = OnceCell::new();
    REDIS_POOL
        .get_or_try_init(|| {
            let config = must_new_redis_config();
            let p = Pool::builder(Manager::new(config.nodes[0].as_str()).unwrap());
            p.config(PoolConfig {
                max_size: config.pool_size as usize,
                timeouts: deadpool_redis::Timeouts {
                    wait: Some(config.wait_timeout),
                    create: Some(config.connection_timeout),
                    recycle: Some(config.recycle_timeout),
                },
                ..Default::default()
            })
            .runtime(Runtime::Tokio1)
            .build()
        })
        .unwrap()
}
