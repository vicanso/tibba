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

use super::{
    ClusterBuildSnafu, ClusterConnectSnafu, Error, RedisSnafu, SingleBuildSnafu,
    SingleConnectSnafu, new_redis_config,
};
use deadpool_redis::cluster::Hook as ClusterHook;
use deadpool_redis::{Hook, HookError, Metrics, PoolConfig, Timeouts};
use redis::aio::ConnectionLike;
use redis::{Arg, Cmd, Pipeline, RedisFuture, Value};
use snafu::ResultExt;
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tibba_config::Config;
use tracing::info;

use super::LOG_TARGET;

type Result<T> = std::result::Result<T, Error>;
/// Return type for `pre_recycle`: compatible with both `Hook` and `ClusterHook`
/// because both managers declare `type Error = redis::RedisError`, so their
/// `HookError` re-exports resolve to the same concrete type.
type HookResult = std::result::Result<(), HookError>;

#[derive(Debug, Default)]
pub struct RedisCmdStat {
    pub cmd: String,
    pub elapsed: Duration,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct RedisStat {
    pub pool_max_size: usize,
    pub pool_size: usize,
    pub pool_available: usize,
    pub pool_waiting: usize,
    pub conn_created: u64,
    pub conn_recycled: u64,
    /// Connections dropped due to idle timeout.
    pub conn_idle_timeout_dropped: u64,
    /// Connections dropped due to max age.
    pub conn_max_age_dropped: u64,
}

pub type RedisCmdStatCallback = dyn Fn(RedisCmdStat) + Send + Sync;

/// Redis connection pool enum that supports both single node and cluster configurations
#[derive(Clone)]
enum RedisPool {
    /// Single Redis node connection pool
    Single(deadpool_redis::Pool),
    /// Redis cluster connection pool
    Cluster(deadpool_redis::cluster::Pool),
}

#[derive(Clone)]
pub struct RedisClient {
    pool: RedisPool,
    stat_callback: Option<&'static RedisCmdStatCallback>,
    hook_stat: HookStat,
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
            RedisPool::Single(p) => Box::new(p.get().await.context(SingleConnectSnafu)?),
            RedisPool::Cluster(p) => Box::new(p.get().await.context(ClusterConnectSnafu)?),
        };

        Ok(RedisClientConn {
            conn,
            stat_callback: self.stat_callback,
        })
    }
    #[must_use]
    pub fn with_stat_callback(mut self, callback: &'static RedisCmdStatCallback) -> Self {
        self.stat_callback = Some(callback);
        self
    }
    /// Gets the status of the pool
    /// # Returns
    /// * `Status` - The status of the pool
    pub fn stat(&self) -> RedisStat {
        let status = match &self.pool {
            RedisPool::Single(p) => p.status(),
            RedisPool::Cluster(p) => p.status(),
        };
        let inner = &self.hook_stat.inner;
        RedisStat {
            pool_max_size: status.max_size,
            pool_size: status.size,
            pool_available: status.available,
            pool_waiting: status.waiting,
            conn_created: inner.created.load(Ordering::Relaxed),
            conn_recycled: inner.recycled.load(Ordering::Relaxed),
            conn_idle_timeout_dropped: inner.idle_timeout_dropped.load(Ordering::Relaxed),
            conn_max_age_dropped: inner.max_age_dropped.load(Ordering::Relaxed),
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
    /// Returns true if connected to a Redis cluster, false for single-node
    pub fn is_cluster(&self) -> bool {
        matches!(self.pool, RedisPool::Cluster(_))
    }
}

#[inline]
fn get_command_name(cmd: &Cmd) -> &str {
    if let Some(Arg::Simple(val)) = cmd.args_iter().next()
        && let Ok(s) = std::str::from_utf8(val)
    {
        return s;
    }
    "unknown"
}

#[inline]
fn wrap_with_stat<'a, 'cb, T>(
    name: Cow<'static, str>,
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
            cmd: name.into_owned(),
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

impl ConnectionLike for RedisClientConn {
    /// Executes a packed Redis command
    /// # Arguments
    /// * `cmd` - The Redis command to execute
    /// # Returns
    /// * `RedisFuture<Value>` - Future that resolves to the command result
    fn req_packed_command<'a>(&'a mut self, cmd: &'a Cmd) -> RedisFuture<'a, Value> {
        if let Some(cb) = self.stat_callback {
            let name = Cow::Owned(get_command_name(cmd).to_owned());
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
            wrap_with_stat(Cow::Borrowed("pipeline"), fut, cb)
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

/// Shared state behind `HookStat`.
///
/// Holds fields that all hook closures and `RedisClient` access together.
/// Future counters (e.g. `AtomicU64`) belong here so every clone sees the
/// same value without extra synchronisation overhead.
struct HookStatInner {
    created: AtomicU64,
    recycled: AtomicU64,
    /// Connections dropped because idle time exceeded `idle_timeout`.
    idle_timeout_dropped: AtomicU64,
    /// Connections dropped because total age exceeded `max_conn_age`.
    max_age_dropped: AtomicU64,
}

/// Encapsulates pool lifecycle logging for a named Redis pool.
///
/// Internally `Arc`-backed: cloning is a single reference-count increment,
/// so it is safe and cheap to give one clone to each hook closure while
/// keeping a copy in `RedisClient` — all four share the same `HookStatInner`.
#[derive(Clone)]
pub struct HookStat {
    label: &'static str,
    max_conn_age: Duration,
    idle_timeout: Duration,
    inner: Arc<HookStatInner>,
}

impl HookStat {
    fn new(label: &'static str, max_conn_age: Duration, idle_timeout: Duration) -> Self {
        Self {
            label,
            max_conn_age,
            idle_timeout,
            inner: Arc::new(HookStatInner {
                created: AtomicU64::new(0),
                recycled: AtomicU64::new(0),
                idle_timeout_dropped: AtomicU64::new(0),
                max_age_dropped: AtomicU64::new(0),
            }),
        }
    }
    /// Called after a new physical connection is established.
    fn post_create(&self) {
        self.inner.created.fetch_add(1, Ordering::Relaxed);
        info!(target: LOG_TARGET, label = self.label, "new connection");
    }
    /// Called before a connection is recycled back into the pool.
    ///
    /// Returns `Err(HookError::Continue(None))` to discard the connection
    /// (deadpool will drop it and create fresh ones on demand) when:
    /// - idle time exceeds `idle_timeout` (if non-zero)
    /// - total age exceeds `max_conn_age` (if non-zero)
    fn pre_recycle(&self, metrics: &Metrics) -> HookResult {
        let idle = metrics.last_used();
        if !self.idle_timeout.is_zero() && idle > self.idle_timeout {
            self.inner
                .idle_timeout_dropped
                .fetch_add(1, Ordering::Relaxed);
            info!(
                target: LOG_TARGET,
                label = self.label,
                idle = idle.as_secs(),
                "drop connection: idle timeout exceeded"
            );
            return Err(HookError::message("drop"));
        }
        let age = metrics.age();
        if !self.max_conn_age.is_zero() && age > self.max_conn_age {
            self.inner.max_age_dropped.fetch_add(1, Ordering::Relaxed);
            info!(
                target: LOG_TARGET,
                label = self.label,
                age = age.as_secs(),
                "drop connection: max age exceeded"
            );
            return Err(HookError::message("drop"));
        }
        Ok(())
    }
    /// Called after a connection has been successfully recycled.
    fn post_recycle(&self, metrics: &Metrics) {
        self.inner.recycled.fetch_add(1, Ordering::Relaxed);
        info!(
            target: LOG_TARGET,
            label = self.label,
            age = metrics.age().as_secs(),
            idle = metrics.last_used().as_secs(),
            "recycle connection"
        );
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
    let pool_config = PoolConfig {
        max_size: redis_config.pool_size as usize,
        timeouts: Timeouts {
            wait: Some(redis_config.wait_timeout),
            create: Some(redis_config.connection_timeout),
            recycle: Some(redis_config.recycle_timeout),
        },
        ..Default::default()
    };

    let password = redis_config.password.as_deref().unwrap_or_default();
    let nodes: Vec<_> = redis_config
        .nodes
        .iter()
        .map(|v| {
            if password.is_empty() {
                return v.to_string();
            }
            v.replace(password, "***")
        })
        .collect();

    let is_single = redis_config.nodes.len() <= 1;
    let hook_stat = HookStat::new(
        if is_single { "single" } else { "cluster" },
        redis_config.max_conn_age,
        redis_config.idle_timeout,
    );

    let (pool, hook_stat) = if is_single {
        // Single node configuration
        let mgr =
            deadpool_redis::Manager::new(redis_config.nodes[0].as_str()).context(RedisSnafu {
                category: "new_pool",
            })?;
        let pool = deadpool_redis::Pool::builder(mgr)
            .config(pool_config)
            .runtime(deadpool_redis::Runtime::Tokio1)
            .post_create(Hook::sync_fn({
                let stat = hook_stat.clone();
                move |_, _| {
                    stat.post_create();
                    Ok(())
                }
            }))
            .pre_recycle(Hook::sync_fn({
                let stat = hook_stat.clone();
                move |_, m| stat.pre_recycle(m)
            }))
            .post_recycle(Hook::sync_fn({
                let stat = hook_stat.clone();
                move |_, m| {
                    stat.post_recycle(m);
                    Ok(())
                }
            }))
            .build()
            .context(SingleBuildSnafu)?;
        (RedisPool::Single(pool), hook_stat)
    } else {
        // Cluster configuration
        let mut cfg = deadpool_redis::cluster::Config::from_urls(redis_config.nodes.clone());
        cfg.pool = Some(pool_config);
        let pool = cfg
            .builder()
            .map_err(deadpool_redis::cluster::CreatePoolError::Config)
            .context(ClusterBuildSnafu)?
            .runtime(deadpool_redis::cluster::Runtime::Tokio1)
            .post_create(ClusterHook::sync_fn({
                let stat = hook_stat.clone();
                move |_, _| {
                    stat.post_create();
                    Ok(())
                }
            }))
            .pre_recycle(ClusterHook::sync_fn({
                let stat = hook_stat.clone();
                move |_, m| stat.pre_recycle(m)
            }))
            .post_recycle(ClusterHook::sync_fn({
                let stat = hook_stat.clone();
                move |_, m| {
                    stat.post_recycle(m);
                    Ok(())
                }
            }))
            .build()
            .map_err(deadpool_redis::cluster::CreatePoolError::Build)
            .context(ClusterBuildSnafu)?;
        (RedisPool::Cluster(pool), hook_stat)
    };
    info!(target: LOG_TARGET, nodes = nodes.join(","), "connect to redis");
    Ok(RedisClient {
        pool,
        stat_callback: None,
        hook_stat,
    })
}
