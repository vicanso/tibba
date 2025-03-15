// Copyright 2025 Tree xie.
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

use super::Error;
use async_trait::async_trait;
use redis::aio::ConnectionLike;
use redis::{Cmd, Pipeline, RedisFuture, Value};
use tibba_config::RedisConfig;

type Result<T> = std::result::Result<T, Error>;

pub enum RedisPool {
    Single(deadpool_redis::Pool),
    Cluster(deadpool_redis::cluster::Pool),
}

impl RedisPool {
    pub async fn get(&self) -> Result<RedisConnection> {
        let conn = match self {
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
    // not support db selected
    fn get_db(&self) -> i64 {
        0
    }
}

pub fn new_redis_pool(config: &RedisConfig) -> Result<RedisPool> {
    let pool = if config.nodes.len() <= 1 {
        let p = deadpool_redis::Pool::builder(
            deadpool_redis::Manager::new(config.nodes[0].as_str()).map_err(|e| Error::Redis {
                category: "new_pool".to_string(),
                source: e,
            })?,
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
        let mut cfg = deadpool_redis::cluster::Config::from_urls(config.nodes.clone());
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
    Ok(pool)
}
