use super::{Error, Result};
use crate::config::must_new_redis_config;
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use redis::aio::ConnectionLike;
use redis::{Cmd, Pipeline, RedisFuture, Value};

pub enum RedisPool {
    Single(deadpool_redis::Pool),
    Cluster(deadpool_redis::cluster::Pool),
}

pub enum RedisConnection {
    Single(deadpool_redis::Connection),
    Cluster(deadpool_redis::cluster::Connection),
}

#[async_trait]
impl ConnectionLike for RedisConnection {
    fn req_packed_command<'a>(&'a mut self, cmd: &'a Cmd) -> RedisFuture<'a, Value> {
        match self {
            RedisConnection::Single(c) => c.req_packed_command(cmd),
            RedisConnection::Cluster(c) => c.req_packed_command(cmd),
        }
    }
    fn req_packed_commands<'a>(
        &'a mut self,
        cmd: &'a Pipeline,
        offset: usize,
        count: usize,
    ) -> RedisFuture<'a, Vec<Value>> {
        match self {
            RedisConnection::Single(c) => c.req_packed_commands(cmd, offset, count),
            RedisConnection::Cluster(c) => c.req_packed_commands(cmd, offset, count),
        }
    }
    fn get_db(&self) -> i64 {
        0
    }
}

fn must_get_redis_pool() -> &'static RedisPool {
    static REDIS_POOL: OnceCell<RedisPool> = OnceCell::new();
    REDIS_POOL
        .get_or_try_init(|| {
            let config = must_new_redis_config();
            let pool = if config.nodes.len() <= 1 {
                let p = deadpool_redis::Pool::builder(
                    deadpool_redis::Manager::new(config.nodes[0].as_str()).unwrap(),
                );
                let pool = p
                    .config(deadpool_redis::PoolConfig {
                        max_size: config.pool_size as usize,
                        timeouts: deadpool_redis::Timeouts {
                            wait: Some(config.wait_timeout),
                            create: Some(config.connection_timeout),
                            recycle: Some(config.recycle_timeout),
                        },
                        ..Default::default()
                    })
                    .runtime(deadpool_redis::Runtime::Tokio1)
                    .build()
                    .map_err(|e| Error::SingleBuild { source: e })?;
                RedisPool::Single(pool)
            } else {
                let mut cfg = deadpool_redis::cluster::Config::from_urls(config.nodes);
                cfg.pool = Some(deadpool_redis::cluster::PoolConfig {
                    max_size: config.pool_size as usize,
                    timeouts: deadpool_redis::Timeouts {
                        wait: Some(config.wait_timeout),
                        create: Some(config.connection_timeout),
                        recycle: Some(config.recycle_timeout),
                    },
                    ..Default::default()
                });
                let pool = cfg
                    .create_pool(Some(deadpool_redis::cluster::Runtime::Tokio1))
                    .map_err(|e| Error::ClusterBuild { source: e })?;
                RedisPool::Cluster(pool)
            };
            Ok::<RedisPool, Error>(pool)
        })
        .unwrap()
}

pub async fn must_get_redis_connection() -> Result<RedisConnection> {
    let conn = match must_get_redis_pool() {
        RedisPool::Single(p) => {
            let conn = p.get().await.map_err(|e| Error::Common {
                category: "connection".to_string(),
                message: e.to_string(),
            })?;
            RedisConnection::Single(conn)
        }
        RedisPool::Cluster(p) => {
            let conn = p.get().await.map_err(|e| Error::Common {
                category: "connection".to_string(),
                message: e.to_string(),
            })?;
            RedisConnection::Cluster(conn)
        }
    };
    Ok(conn)
}
