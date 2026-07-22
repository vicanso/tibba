// Copyright 2026 Tree xie.
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
use deadpool::managed::{self, HookError, Metrics, PoolConfig, RecycleError, Timeouts};
use redis::aio::{ConnectionLike, MultiplexedConnection};
use redis::cluster::ClusterClient;
use redis::cluster_async::ClusterConnection;
use redis::{
    Arg, AsyncConnectionConfig, Client as RedisRawClient, Cmd, Pipeline, RedisError, RedisFuture,
    RedisResult, Value,
};
use snafu::ResultExt;
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tibba_config::Config;
use tracing::info;

use super::LOG_TARGET;

type Result<T> = std::result::Result<T, Error>;

/// `pre_recycle` 的返回类型；单节点与集群 Manager 的 `Error` 同为 `RedisError`，共用此别名。
type HookResult = std::result::Result<(), HookError<RedisError>>;

#[derive(Debug, Default)]
pub struct RedisCmdStat {
    pub cmd: String,
    pub elapsed: Duration,
    pub error: Option<String>,
    /// 意图性阻塞命令（BRPOP 等）：慢命令告警应忽略，避免淹没真实慢查询。
    pub intentional_block: bool,
}

#[derive(Debug, Default)]
pub struct RedisStat {
    pub pool_max_size: usize,
    pub pool_size: usize,
    pub pool_available: usize,
    pub pool_waiting: usize,
    pub conn_created: u64,
    pub conn_recycled: u64,
    /// 因空闲超时而丢弃的连接数
    pub conn_idle_timeout_dropped: u64,
    /// 因超过最大存活时间而丢弃的连接数
    pub conn_max_age_dropped: u64,
}

pub type RedisCmdStatCallback = dyn Fn(RedisCmdStat) + Send + Sync;

/// Redis 连接池枚举，支持单节点和集群两种模式。
///
/// 两种模式都用自建 Manager：deadpool-redis 自带的 Manager 走
/// `get_multiplexed_async_connection()` / `ClusterClient::new()`，无法注入
/// `response_timeout`，池内连接只能吃 redis-rs 默认的 500ms。
#[derive(Clone)]
enum RedisPool {
    /// 单节点：建连时应用 URI `response_timeout`
    Single(managed::Pool<TimeoutAwareManager>),
    /// 集群：建 `ClusterClient` 时应用 URI `response_timeout`
    Cluster(managed::Pool<TimeoutAwareClusterManager>),
}

#[derive(Clone)]
pub struct RedisClient {
    pool: RedisPool,
    /// 建池时的节点 URL 列表；专用连接（不归池）直接按此打开，不经过 deadpool。
    nodes: Vec<String>,
    /// 池化连接的 response timeout（URI `response_timeout`；`None` = 不超时）。
    response_timeout: Option<Duration>,
    /// 建连 timeout（与 URI `connection_timeout` 一致）。
    connection_timeout: Duration,
    /// 慢命令阈值（URI `slow`）。
    slow_cmd_threshold: Duration,
    stat_callback: Option<&'static RedisCmdStatCallback>,
    hook_stat: HookStat,
}

/// 单节点 Manager：在 create 时注入 response_timeout，覆盖 redis-rs 默认 500ms。
struct TimeoutAwareManager {
    client: RedisRawClient,
    response_timeout: Option<Duration>,
    connection_timeout: Option<Duration>,
    ping_number: AtomicUsize,
}

impl managed::Manager for TimeoutAwareManager {
    type Type = MultiplexedConnection;
    type Error = RedisError;

    async fn create(&self) -> RedisResult<MultiplexedConnection> {
        let cfg = AsyncConnectionConfig::new()
            .set_response_timeout(self.response_timeout)
            .set_connection_timeout(self.connection_timeout);
        self.client
            .get_multiplexed_async_connection_with_config(&cfg)
            .await
    }

    async fn recycle(
        &self,
        conn: &mut MultiplexedConnection,
        _: &Metrics,
    ) -> managed::RecycleResult<RedisError> {
        let ping_number = self.ping_number.fetch_add(1, Ordering::Relaxed).to_string();
        // 与 deadpool-redis 一致：UNWATCH + PING 合并成一次 round trip
        let (n,) = redis::Pipeline::with_capacity(2)
            .cmd("UNWATCH")
            .ignore()
            .cmd("PING")
            .arg(&ping_number)
            .query_async::<(String,)>(conn)
            .await?;
        if n == ping_number {
            Ok(())
        } else {
            Err(RecycleError::message("Invalid PING response"))
        }
    }
}

/// 集群 Manager：在 `ClusterClient` 上应用 URI `response_timeout` / `connection_timeout`。
struct TimeoutAwareClusterManager {
    client: ClusterClient,
    ping_number: AtomicUsize,
}

impl managed::Manager for TimeoutAwareClusterManager {
    type Type = ClusterConnection;
    type Error = RedisError;

    async fn create(&self) -> RedisResult<ClusterConnection> {
        self.client.get_async_connection().await
    }

    async fn recycle(
        &self,
        conn: &mut ClusterConnection,
        _: &Metrics,
    ) -> managed::RecycleResult<RedisError> {
        let ping_number = self.ping_number.fetch_add(1, Ordering::Relaxed).to_string();
        // 集群连接不支持 UNWATCH 的跨槽 pipeline，只 PING
        let n = redis::cmd("PING")
            .arg(&ping_number)
            .query_async::<String>(conn)
            .await?;
        if n == ping_number {
            Ok(())
        } else {
            Err(RecycleError::message("Invalid PING response"))
        }
    }
}

/// deadpool `Object` → `ConnectionLike` 的转发层。
///
/// `managed::Object<M>` 只 `Deref` 到 `M::Type`，本身不实现 `ConnectionLike`
/// （deadpool-redis 同样靠自己的 `Connection` newtype 转发）；自建 Manager 后需自行补齐。
struct PooledConn<M: managed::Manager>(managed::Object<M>);

impl<M> ConnectionLike for PooledConn<M>
where
    M: managed::Manager,
    M::Type: ConnectionLike,
{
    fn req_packed_command<'a>(&'a mut self, cmd: &'a Cmd) -> RedisFuture<'a, Value> {
        self.0.req_packed_command(cmd)
    }

    fn req_packed_commands<'a>(
        &'a mut self,
        cmd: &'a Pipeline,
        offset: usize,
        count: usize,
    ) -> RedisFuture<'a, Vec<Value>> {
        self.0.req_packed_commands(cmd, offset, count)
    }

    fn get_db(&self) -> i64 {
        self.0.get_db()
    }
}

/// 池化连接：`Drop` 时归还 deadpool。
///
/// 仅适合短命令（GET/SET/INCR…）。**禁止**在此连接上跑会无限期阻塞的命令
/// （如 `BRPOP` 无超时 / `SUBSCRIBE`），否则会占死 pool slot。
pub struct RedisClientConn {
    conn: Box<dyn ConnectionLike + Send + Sync>,
    stat_callback: Option<&'static RedisCmdStatCallback>,
}

/// redis-rs 1.x 默认 `DEFAULT_RESPONSE_TIMEOUT = 500ms`。
/// 若专用连接仍用该默认值，`BRPOP` 阻塞 ≥1s 必被客户端误判为 `timed out`。
/// 见 [`RedisClient::dedicated_blocking_conn`]：把服务端最长阻塞时长做进签名，并自动推导 response timeout。
const BLOCKING_RESPONSE_SAFETY_MARGIN: Duration = Duration::from_secs(1);

/// 非阻塞专用写连接（reply_loop）的默认客户端 response timeout。
/// 显式设置，避免依赖 redis 默认 500ms；写路径应远小于此值。
const DEDICATED_COMMAND_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// 专用连接：不进入连接池，`Drop` 时关闭 TCP。
///
/// 创建时须通过 [`RedisClient::dedicated_blocking_conn`]（阻塞读）或
/// [`RedisClient::dedicated_command_conn`]（短写），二者都会**显式**配置
/// response timeout，不会落入 redis 默认 500ms。
pub struct RedisDedicatedConn {
    conn: Box<dyn ConnectionLike + Send + Sync>,
    stat_callback: Option<&'static RedisCmdStatCallback>,
    /// 创建时约定的最长阻塞（阻塞连接为 `Some`；短命令连接为 `None`）。
    max_block: Option<Duration>,
}

impl RedisDedicatedConn {
    /// 创建本连接时声明的最长阻塞时长（仅 [`RedisClient::dedicated_blocking_conn`] 有值）。
    #[must_use]
    pub fn max_block(&self) -> Option<Duration> {
        self.max_block
    }
}

/// 由服务端阻塞命令的最长等待时间，推导客户端 `response_timeout`。
///
/// | `max_block` | 对应 BRPOP timeout | 客户端 response_timeout |
/// |-------------|--------------------|-------------------------|
/// | `Duration::ZERO` | `BRPOP key 0`（无限等） | `None`（关闭超时） |
/// | `d > 0` | `BRPOP key secs` | `d + 1s` 安全裕量 |
///
/// 裕量保证：服务端在 `d` 返回 nil/元素后，客户端不会因默认 500ms 先超时。
#[must_use]
pub fn response_timeout_for_max_block(max_block: Duration) -> Option<Duration> {
    if max_block.is_zero() {
        None
    } else {
        Some(max_block.saturating_add(BLOCKING_RESPONSE_SAFETY_MARGIN))
    }
}

/// `BRPOP` / `BZPOPMIN` 等的 timeout 秒数（与 [`RedisClient::dedicated_blocking_conn`] 的 `max_block` 对齐）。
///
/// - `Duration::ZERO` → `0`（服务端无限等）
/// - 否则向上取整到整秒，且至少为 `1`（避免 `as_secs()==0` 被 Redis 当成无限等）
#[must_use]
pub fn brpop_timeout_secs(max_block: Duration) -> u64 {
    if max_block.is_zero() {
        0
    } else {
        // ceil 到秒
        let secs = max_block.as_secs();
        let ceil = secs.saturating_add(u64::from(max_block.subsec_nanos() > 0));
        ceil.max(1)
    }
}

impl RedisClient {
    /// 从连接池借用一个连接（短生命周期命令用）。
    ///
    /// 连接在 `RedisClientConn` drop 时归还池。**禁止**用于 `BRPOP` 等长阻塞命令，
    /// 请用 [`Self::dedicated_blocking_conn`]。
    #[inline]
    pub async fn conn(&self) -> Result<RedisClientConn> {
        let conn: Box<dyn ConnectionLike + Send + Sync> = match &self.pool {
            RedisPool::Single(p) => {
                Box::new(PooledConn(p.get().await.context(SingleConnectSnafu)?))
            }
            RedisPool::Cluster(p) => {
                Box::new(PooledConn(p.get().await.context(ClusterConnectSnafu)?))
            }
        };

        Ok(RedisClientConn {
            conn,
            stat_callback: self.stat_callback,
        })
    }

    /// 打开**阻塞读**专用连接（不归池），并把「最长阻塞时长」做进签名。
    ///
    /// # 为何必须传 `max_block`
    /// redis-rs 1.x 默认 response timeout **仅 500ms**。若只解决「不归池」却不改超时，
    /// `BRPOP` 阻塞 2s 仍会得到 `error=timed out`。因此本 API **强制**声明服务端最长等待，
    /// 并自动设置客户端 `response_timeout = max_block + 1s`（`max_block == 0` 则关闭超时）。
    ///
    /// # 参数
    /// - `max_block`：与 `BRPOP key <secs>` 对齐
    ///   - `Duration::ZERO` → 无限阻塞（`BRPOP … 0`），客户端 timeout = `None`
    ///   - 其它 → 用 [`brpop_timeout_secs`] 得到 secs，客户端 timeout = 该时长 + 1s 裕量
    ///
    /// # 示例（ferry pull_loop）
    /// ```ignore
    /// let max_block = Duration::from_secs(2);
    /// let mut pull = client.dedicated_blocking_conn(max_block).await?;
    /// let secs = brpop_timeout_secs(max_block); // 2
    /// let item: Option<(String, String)> = redis::cmd("BRPOP")
    ///     .arg("ferry:q").arg(secs).query_async(&mut pull).await?;
    /// ```
    pub async fn dedicated_blocking_conn(&self, max_block: Duration) -> Result<RedisDedicatedConn> {
        let response_timeout = response_timeout_for_max_block(max_block);
        self.open_dedicated(response_timeout, Some(max_block)).await
    }

    /// 打开**短命令**专用连接（不归池），用于 ferry `reply_loop` 等只写/短读路径。
    ///
    /// response timeout 优先用 URI `response_timeout`，否则 5s（显式设置，非 redis 默认 500ms）。
    pub async fn dedicated_command_conn(&self) -> Result<RedisDedicatedConn> {
        let t = self
            .response_timeout
            .unwrap_or(DEDICATED_COMMAND_RESPONSE_TIMEOUT);
        // URI 配 0（None）时短写仍给 5s 上限，避免无限挂死；要无限请用 blocking API
        let t = if t.is_zero() {
            DEDICATED_COMMAND_RESPONSE_TIMEOUT
        } else {
            t
        };
        self.open_dedicated(Some(t), None).await
    }

    /// URI 配置的客户端 response timeout（池化连接建连时已应用）。
    #[must_use]
    pub fn response_timeout(&self) -> Option<Duration> {
        self.response_timeout
    }

    /// URI `slow=` 慢命令阈值（应用侧 `stat_callback` 用）。
    #[must_use]
    pub fn slow_cmd_threshold(&self) -> Duration {
        self.slow_cmd_threshold
    }

    /// 同 [`Self::dedicated_command_conn`]，自定义 response timeout（必须 `> 0`）。
    pub async fn dedicated_command_conn_with_timeout(
        &self,
        response_timeout: Duration,
    ) -> Result<RedisDedicatedConn> {
        if response_timeout.is_zero() {
            return Err(Error::Redis {
                category: "dedicated_command".to_string(),
                source: redis::RedisError::from((
                    redis::ErrorKind::InvalidClientConfig,
                    "dedicated_command_conn response_timeout must be > 0; \
                     for infinite blocking use dedicated_blocking_conn(Duration::ZERO)",
                )),
            });
        }
        self.open_dedicated(Some(response_timeout), None).await
    }

    /// 内部：按显式 response_timeout 建专用连接。
    async fn open_dedicated(
        &self,
        response_timeout: Option<Duration>,
        max_block: Option<Duration>,
    ) -> Result<RedisDedicatedConn> {
        let conn: Box<dyn ConnectionLike + Send + Sync> = if self.nodes.len() <= 1 {
            let url = self.nodes.first().ok_or_else(|| Error::Redis {
                category: "dedicated_open".to_string(),
                source: redis::RedisError::from((
                    redis::ErrorKind::InvalidClientConfig,
                    "no redis node configured",
                )),
            })?;
            let client = RedisRawClient::open(url.as_str()).context(RedisSnafu {
                category: "dedicated_open",
            })?;
            // 必须 set_response_timeout：AsyncConnectionConfig::new() 默认仍是 500ms
            let cfg = AsyncConnectionConfig::new()
                .set_response_timeout(response_timeout)
                .set_connection_timeout(Some(self.connection_timeout));
            let c = client
                .get_multiplexed_async_connection_with_config(&cfg)
                .await
                .context(RedisSnafu {
                    category: "dedicated_connect",
                })?;
            Box::new(c)
        } else {
            let mut builder = ClusterClient::builder(self.nodes.clone())
                .connection_timeout(self.connection_timeout);
            if let Some(d) = response_timeout {
                builder = builder.response_timeout(d);
            } else {
                builder = builder.overall_response_timeout(None);
            }
            let cluster = builder.build().context(RedisSnafu {
                category: "dedicated_cluster_open",
            })?;
            let c = cluster.get_async_connection().await.context(RedisSnafu {
                category: "dedicated_cluster_connect",
            })?;
            Box::new(c)
        };

        info!(
            target: LOG_TARGET,
            label = self.hook_stat.label,
            cluster = self.is_cluster(),
            max_block_ms = max_block.map(|d| d.as_millis()),
            response_timeout_ms = response_timeout.map(|d| d.as_millis()),
            "open dedicated redis connection (not pooled)"
        );

        Ok(RedisDedicatedConn {
            conn,
            stat_callback: self.stat_callback,
            max_block,
        })
    }

    /// 设置命令统计回调，支持链式调用。
    #[must_use]
    pub fn with_stat_callback(mut self, callback: &'static RedisCmdStatCallback) -> Self {
        self.stat_callback = Some(callback);
        self
    }

    /// 获取连接池状态统计信息。
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

    /// 关闭连接池（将连接数收缩至 0）。
    pub fn close(&self) {
        match &self.pool {
            RedisPool::Single(p) => p.close(),
            RedisPool::Cluster(p) => p.close(),
        }
    }

    /// 是否为集群模式。
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

/// 意图性阻塞 / 长等命令：耗时不计入「慢命令」告警（仍可上报 error）。
///
/// 含 list/zset/stream 阻塞读与 pubsub 订阅族；`XREAD`/`XREADGROUP` 仅当参数含 `BLOCK`。
#[must_use]
pub fn is_intentional_blocking_command(cmd: &Cmd) -> bool {
    let name = get_command_name(cmd);
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "BRPOP" | "BLPOP" | "BRPOPLPUSH" | "BLMOVE" | "BLMPOP" | "BZPOPMIN" | "BZPOPMAX"
        | "BZMPOP" | "SUBSCRIBE" | "PSUBSCRIBE" | "SSUBSCRIBE" | "WAIT" => true,
        "XREAD" | "XREADGROUP" => cmd.args_iter().any(|a| match a {
            Arg::Simple(b) => b.eq_ignore_ascii_case(b"BLOCK"),
            _ => false,
        }),
        _ => false,
    }
}

#[inline]
fn wrap_with_stat<'a, 'cb, T>(
    name: Cow<'static, str>,
    intentional_block: bool,
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
        // 阻塞命令正常路径：跳过 stat_callback，避免刷慢命令榜
        // 出错时仍上报（连接断开等需要看见）
        let is_err = res.is_err();
        if intentional_block && !is_err {
            return res;
        }
        let mut stat = RedisCmdStat {
            cmd: name.into_owned(),
            elapsed,
            intentional_block,
            ..Default::default()
        };
        if let Err(e) = &res {
            stat.error = Some(e.to_string());
        }
        callback(stat);
        res
    })
}

macro_rules! impl_connection_like {
    ($ty:ty) => {
        impl ConnectionLike for $ty {
            /// 执行单条 Redis 命令，若设置了统计回调则记录耗时与错误。
            fn req_packed_command<'a>(&'a mut self, cmd: &'a Cmd) -> RedisFuture<'a, Value> {
                if let Some(cb) = self.stat_callback {
                    let name = Cow::Owned(get_command_name(cmd).to_owned());
                    let block = is_intentional_blocking_command(cmd);
                    let fut = self.conn.req_packed_command(cmd);
                    wrap_with_stat(name, block, fut, cb)
                } else {
                    self.conn.req_packed_command(cmd)
                }
            }

            /// 以 pipeline 批量执行 Redis 命令，若设置了统计回调则整体计时。
            fn req_packed_commands<'a>(
                &'a mut self,
                cmd: &'a Pipeline,
                offset: usize,
                count: usize,
            ) -> RedisFuture<'a, Vec<Value>> {
                if let Some(cb) = self.stat_callback {
                    // pipeline 不按单命令拆分；整体计时（非 BRPOP 单命令场景）
                    let fut = self.conn.req_packed_commands(cmd, offset, count);
                    wrap_with_stat(Cow::Borrowed("pipeline"), false, fut, cb)
                } else {
                    self.conn.req_packed_commands(cmd, offset, count)
                }
            }

            /// 获取当前数据库编号，集群模式固定返回 0（不支持多 DB）。
            fn get_db(&self) -> i64 {
                0
            }
        }
    };
}

impl_connection_like!(RedisClientConn);
impl_connection_like!(RedisDedicatedConn);

/// HookStat 的内部共享状态，通过原子计数器记录连接生命周期事件。
/// 所有 hook 闭包与 RedisClient 共享同一份实例。
struct HookStatInner {
    created: AtomicU64,
    recycled: AtomicU64,
    /// 因空闲超时而丢弃的连接数
    idle_timeout_dropped: AtomicU64,
    /// 因超过最大存活时间而丢弃的连接数
    max_age_dropped: AtomicU64,
}

/// 封装连接池生命周期日志与统计。
/// 内部通过 Arc 共享，克隆开销极低，可安全分发给各 hook 闭包。
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

    /// 新物理连接建立后回调，累计创建计数并打印日志。
    fn post_create(&self) {
        self.inner.created.fetch_add(1, Ordering::Relaxed);
        info!(target: LOG_TARGET, label = self.label, "new connection");
    }

    /// 连接回池前回调。超过空闲时限或最大存活时限时丢弃连接并返回 Err。
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

    /// 连接成功回池后回调，累计复用计数并打印日志。
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

/// 给 deadpool builder 挂上生命周期 hook（单节点 / 集群两种 Manager 共用）。
///
/// `managed::Hook<M>` 随 Manager 泛型变化，闭包无法跨类型复用，故以宏展开。
macro_rules! attach_pool_hooks {
    ($builder:expr, $stat:expr) => {
        $builder
            .post_create(managed::Hook::sync_fn({
                let stat = $stat.clone();
                move |_, _| {
                    stat.post_create();
                    Ok(())
                }
            }))
            .pre_recycle(managed::Hook::sync_fn({
                let stat = $stat.clone();
                move |_, m| stat.pre_recycle(m)
            }))
            .post_recycle(managed::Hook::sync_fn({
                let stat = $stat.clone();
                move |_, m| {
                    stat.post_recycle(m);
                    Ok(())
                }
            }))
    };
}

/// 根据配置创建 Redis 客户端（单节点或集群）。
///
/// 两种模式均使用自建 Manager，确保 URI `response_timeout` 对**池内**连接同样生效
/// （deadpool-redis 自带 Manager 不暴露该配置，池内连接会退回 redis-rs 默认 500ms）。
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
        // 单节点：TimeoutAwareManager 在 create 时写入 response_timeout（覆盖默认 500ms）
        let raw = RedisRawClient::open(redis_config.nodes[0].as_str()).context(RedisSnafu {
            category: "new_pool",
        })?;
        let mgr = TimeoutAwareManager {
            client: raw,
            response_timeout: redis_config.response_timeout,
            connection_timeout: Some(redis_config.connection_timeout),
            ping_number: AtomicUsize::new(0),
        };
        let builder = managed::Pool::builder(mgr)
            .config(pool_config)
            .runtime(deadpool::Runtime::Tokio1);
        let pool = attach_pool_hooks!(builder, hook_stat)
            .build()
            .context(SingleBuildSnafu)?;
        (RedisPool::Single(pool), hook_stat)
    } else {
        // 集群：ClusterClient 上直接配 response_timeout（同时作为含重试的整体超时上限）
        let mut cluster_builder = ClusterClient::builder(redis_config.nodes.clone())
            .connection_timeout(redis_config.connection_timeout);
        cluster_builder = match redis_config.response_timeout {
            Some(d) => cluster_builder.response_timeout(d),
            // 配 0 表示不超时：单次响应与整体都不设上限
            None => cluster_builder.overall_response_timeout(None),
        };
        let client = cluster_builder.build().context(RedisSnafu {
            category: "new_cluster_pool",
        })?;
        let mgr = TimeoutAwareClusterManager {
            client,
            ping_number: AtomicUsize::new(0),
        };
        let builder = managed::Pool::builder(mgr)
            .config(pool_config)
            .runtime(deadpool::Runtime::Tokio1);
        let pool = attach_pool_hooks!(builder, hook_stat)
            .build()
            .context(ClusterBuildSnafu)?;
        (RedisPool::Cluster(pool), hook_stat)
    };
    info!(
        target: LOG_TARGET,
        nodes = nodes.join(","),
        response_timeout_ms = redis_config.response_timeout.map(|d| d.as_millis()),
        slow_cmd_threshold_ms = redis_config.slow_cmd_threshold.as_millis(),
        "connect to redis"
    );
    Ok(RedisClient {
        pool,
        nodes: redis_config.nodes,
        response_timeout: redis_config.response_timeout,
        connection_timeout: redis_config.connection_timeout,
        slow_cmd_threshold: redis_config.slow_cmd_threshold,
        stat_callback: None,
        hook_stat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_timeout_infinite_when_max_block_zero() {
        assert_eq!(response_timeout_for_max_block(Duration::ZERO), None);
        assert_eq!(brpop_timeout_secs(Duration::ZERO), 0);
    }

    #[test]
    fn response_timeout_adds_safety_margin() {
        let max_block = Duration::from_secs(2);
        assert_eq!(
            response_timeout_for_max_block(max_block),
            Some(Duration::from_secs(3)),
            "2s BRPOP 必须配 >2s 的客户端超时，否则踩 redis 默认 500ms"
        );
        assert_eq!(brpop_timeout_secs(max_block), 2);
    }

    #[test]
    fn brpop_timeout_secs_ceils_subsec() {
        assert_eq!(brpop_timeout_secs(Duration::from_millis(500)), 1);
        assert_eq!(brpop_timeout_secs(Duration::from_millis(1500)), 2);
    }

    #[test]
    fn blocking_commands_are_exempt_from_slow_stat() {
        for name in [
            "BRPOP",
            "BLPOP",
            "BRPOPLPUSH",
            "BLMOVE",
            "BLMPOP",
            "BZPOPMIN",
            "BZPOPMAX",
            "BZMPOP",
            "SUBSCRIBE",
            "PSUBSCRIBE",
            "SSUBSCRIBE",
            "WAIT",
        ] {
            assert!(
                is_intentional_blocking_command(&redis::cmd(name)),
                "{name} 应豁免慢命令统计"
            );
            // 命令名大小写不敏感
            assert!(is_intentional_blocking_command(&redis::cmd(
                &name.to_ascii_lowercase()
            )));
        }
    }

    #[test]
    fn normal_commands_are_not_exempt() {
        for name in ["GET", "SET", "INCR", "SCAN", "EVALSHA", "LPUSH", "RPOP"] {
            assert!(
                !is_intentional_blocking_command(&redis::cmd(name)),
                "{name} 不应被当成阻塞命令"
            );
        }
    }

    #[test]
    fn xread_is_exempt_only_with_block_arg() {
        // 无 BLOCK：普通读，正常计入慢命令
        let mut plain = redis::cmd("XREAD");
        plain.arg("COUNT").arg(10).arg("STREAMS").arg("s").arg("0");
        assert!(!is_intentional_blocking_command(&plain));

        // 带 BLOCK：意图性阻塞，豁免
        let mut blocking = redis::cmd("XREADGROUP");
        blocking
            .arg("BLOCK")
            .arg(2000)
            .arg("STREAMS")
            .arg("s")
            .arg(">");
        assert!(is_intentional_blocking_command(&blocking));
    }
}
