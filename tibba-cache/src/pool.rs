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

/// Redis connection pool enum that supports both single node and cluster configurations
pub enum RedisPool {
    /// Single Redis node connection pool
    Single(deadpool_redis::Pool),
    /// Redis cluster connection pool
    Cluster(deadpool_redis::cluster::Pool),
}

impl RedisPool {
    /// Gets a connection from the pool
    /// # Returns
    /// * `Ok(RedisConnection)` - A connection wrapper that works with both single and cluster modes
    /// * `Err(Error)` - Failed to get connection from pool
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

/// Connection wrapper that supports both single node and cluster connections
pub enum RedisConnection {
    /// Single Redis node connection
    Single(deadpool_redis::Connection),
    /// Redis cluster connection
    Cluster(deadpool_redis::cluster::Connection),
}

#[async_trait]
impl ConnectionLike for RedisConnection {
    /// Executes a packed Redis command
    /// # Arguments
    /// * `cmd` - The Redis command to execute
    /// # Returns
    /// * `RedisFuture<Value>` - Future that resolves to the command result
    fn req_packed_command<'a>(&'a mut self, cmd: &'a Cmd) -> RedisFuture<'a, Value> {
        match self {
            RedisConnection::Single(c) => c.req_packed_command(cmd),
            RedisConnection::Cluster(c) => c.req_packed_command(cmd),
        }
    }

    /// Executes multiple packed Redis commands in a pipeline
    /// # Arguments
    /// * `cmd` - The pipeline of Redis commands
    /// * `offset` - Starting offset in the pipeline
    /// * `count` - Number of commands to execute
    /// # Returns
    /// * `RedisFuture<Vec<Value>>` - Future that resolves to multiple command results
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

    /// Gets the current Redis database number
    /// # Notes
    /// * Always returns 0 as database selection is not supported in cluster mode
    /// # Returns
    /// * `i64` - The database number (always 0)
    fn get_db(&self) -> i64 {
        0
    }
}

/// Creates a new Redis connection pool based on configuration
/// # Arguments
/// * `config` - Redis configuration including connection details and pool settings
/// # Returns
/// * `Ok(RedisPool)` - Successfully created pool (single or cluster)
/// * `Err(Error)` - Failed to create pool
/// # Notes
/// * Creates a single node pool if only one node is configured
/// * Creates a cluster pool if multiple nodes are configured
/// * Configures pool size and various timeouts from the provided config
pub fn new_redis_pool(config: &RedisConfig) -> Result<RedisPool> {
    let pool = if config.nodes.len() <= 1 {
        // Single node configuration
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
                    wait: Some(config.wait_timeout), // Maximum time to wait for connection
                    create: Some(config.connection_timeout), // Maximum time to establish connection
                    recycle: Some(config.recycle_timeout), // Maximum connection lifetime
                },
                ..Default::default()
            })
            .runtime(deadpool_redis::Runtime::Tokio1)
            .build()
            .map_err(|e| Error::SingleBuild { source: e })?;
        RedisPool::Single(pool)
    } else {
        // Cluster configuration
        let mut cfg = deadpool_redis::cluster::Config::from_urls(config.nodes.clone());
        cfg.pool = Some(deadpool_redis::cluster::PoolConfig {
            max_size: config.pool_size as usize,
            timeouts: deadpool_redis::Timeouts {
                wait: Some(config.wait_timeout), // Maximum time to wait for connection
                create: Some(config.connection_timeout), // Maximum time to establish connection
                recycle: Some(config.recycle_timeout), // Maximum connection lifetime
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
