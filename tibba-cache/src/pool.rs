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

use super::{Error, new_redis_config};
use async_trait::async_trait;
use deadpool_redis::{PoolConfig, Status, Timeouts};
use redis::aio::ConnectionLike;
use redis::{Arg, Cmd, Pipeline, RedisFuture, Value};
use std::time::Duration;
use tibba_config::Config;
use tracing::info;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Default)]
pub struct RedisCmdStat {
    pub cmd: String,
    pub elapsed: Duration,
    pub error: Option<String>,
}

pub type RedisCmdStatCallback = dyn Fn(RedisCmdStat) + Send + Sync;

/// Redis connection pool enum that supports both single node and cluster configurations
enum RedisPool {
    /// Single Redis node connection pool
    Single(deadpool_redis::Pool),
    /// Redis cluster connection pool
    Cluster(deadpool_redis::cluster::Pool),
}

pub struct RedisClient {
    pool: RedisPool,
    stat_callback: Option<&'static RedisCmdStatCallback>,
}
pub struct RedisClientConn {
    conn: Box<dyn ConnectionLike + Send + Sync>,
    stat_callback: Option<&'static RedisCmdStatCallback>,
}

impl RedisClient {
    /// Gets a connection from the pool
    /// # Returns
    /// * `Ok(RedisConnection)` - A connection wrapper that works with both single and cluster modes
    /// * `Err(Error)` - Failed to get connection from pool
    #[inline]
    pub async fn conn(&self) -> Result<RedisClientConn> {
        let conn: Box<dyn ConnectionLike + Send + Sync> = match &self.pool {
            RedisPool::Single(p) => Box::new(p.get().await.map_err(|e| Error::Common {
                category: "connection".to_string(),
                message: e.to_string(),
            })?),
            RedisPool::Cluster(p) => Box::new(p.get().await.map_err(|e| Error::Common {
                category: "connection".to_string(),
                message: e.to_string(),
            })?),
        };

        Ok(RedisClientConn {
            conn,
            stat_callback: self.stat_callback,
        })
    }
    pub fn with_stat_callback(&mut self, callback: &'static RedisCmdStatCallback) {
        self.stat_callback = Some(callback);
    }
    /// Gets the status of the pool
    /// # Returns
    /// * `Status` - The status of the pool
    pub fn status(&self) -> Status {
        match &self.pool {
            RedisPool::Single(p) => p.status(),
            RedisPool::Cluster(p) => p.status(),
        }
    }
    /// Closes the pool
    /// # Notes
    /// * This operation resizes the pool to 0
    pub fn close(&self) {
        match &self.pool {
            RedisPool::Single(p) => p.close(),
            RedisPool::Cluster(p) => p.close(),
        }
    }
}

#[inline]
fn get_command_name(cmd: &Cmd) -> String {
    if let Some(Arg::Simple(val)) = cmd.args_iter().next()
        && let Ok(s) = std::str::from_utf8(val)
    {
        return s.to_string();
    }
    "unknown".to_string()
}

#[inline]
fn wrap_with_stat<'a, 'cb, T>(
    name: String,
    fut: RedisFuture<'a, T>,
    callback: &'cb RedisCmdStatCallback,
) -> RedisFuture<'a, T>
where
    T: Send + 'a,
    'cb: 'a,
{
    Box::pin(async move {
        let start = std::time::Instant::now();
        let res = fut.await;
        let elapsed = start.elapsed();
        let mut stat = RedisCmdStat {
            cmd: name,
            elapsed,
            ..Default::default()
        };
        if let Err(e) = &res {
            stat.error = Some(e.to_string());
        }
        callback(stat);
        res
    })
}

#[async_trait]
impl ConnectionLike for RedisClientConn {
    /// Executes a packed Redis command
    /// # Arguments
    /// * `cmd` - The Redis command to execute
    /// # Returns
    /// * `RedisFuture<Value>` - Future that resolves to the command result
    fn req_packed_command<'a>(&'a mut self, cmd: &'a Cmd) -> RedisFuture<'a, Value> {
        if let Some(cb) = self.stat_callback {
            let name = get_command_name(cmd);
            let fut = self.conn.req_packed_command(cmd);
            wrap_with_stat(name, fut, cb)
        } else {
            self.conn.req_packed_command(cmd)
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
        if let Some(cb) = self.stat_callback {
            let fut = self.conn.req_packed_commands(cmd, offset, count);
            wrap_with_stat("pipeline".to_string(), fut, cb)
        } else {
            self.conn.req_packed_commands(cmd, offset, count)
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

fn make_pool_config(redis_config: &super::RedisConfig) -> PoolConfig {
    PoolConfig {
        max_size: redis_config.pool_size as usize,
        timeouts: Timeouts {
            wait: Some(redis_config.wait_timeout),
            create: Some(redis_config.connection_timeout),
            recycle: Some(redis_config.recycle_timeout),
        },
        ..Default::default()
    }
}

/// Creates a new Redis connection pool based on configuration
/// # Arguments
/// * `config` - Redis configuration including connection details and pool settings
/// # Returns
/// * `Ok(RedisClient)` - Successfully created pool (single or cluster)
/// * `Err(Error)` - Failed to create pool
/// # Notes
/// * Creates a single node pool if only one node is configured
/// * Creates a cluster pool if multiple nodes are configured
/// * Configures pool size and various timeouts from the provided config
pub fn new_redis_client(config: &Config) -> Result<RedisClient> {
    let redis_config = new_redis_config(config)?;
    let pool_config = make_pool_config(&redis_config);

    let password = redis_config.password.clone().unwrap_or_default();
    let nodes: Vec<_> = redis_config
        .nodes
        .clone()
        .iter()
        .map(|v| {
            if password.is_empty() {
                return v.to_string();
            }
            v.replace(&password, "***")
        })
        .collect();

    let pool = if redis_config.nodes.len() <= 1 {
        // Single node configuration
        let mgr = deadpool_redis::Manager::new(redis_config.nodes[0].as_str()).map_err(|e| {
            Error::Redis {
                category: "new_pool".to_string(),
                source: e,
            }
        })?;
        let pool = deadpool_redis::Pool::builder(mgr)
            .config(pool_config)
            .runtime(deadpool_redis::Runtime::Tokio1)
            .build()
            .map_err(|e| Error::SingleBuild { source: e })?;
        RedisPool::Single(pool)
    } else {
        // Cluster configuration
        let mut cfg = deadpool_redis::cluster::Config::from_urls(redis_config.nodes.clone());
        cfg.pool = Some(pool_config);
        let pool = cfg
            .create_pool(Some(deadpool_redis::cluster::Runtime::Tokio1))
            .map_err(|e| Error::ClusterBuild { source: e })?;
        RedisPool::Cluster(pool)
    };
    info!(
        category = "redis",
        nodes = nodes.join(","),
        "connect to redis"
    );
    Ok(RedisClient {
        pool,
        stat_callback: None,
    })
}
