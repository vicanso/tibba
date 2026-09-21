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

//! Redis 缓存与连接池。
//!
//! ## 热点路径约定（调用方应优先复用本 crate）
//! | 场景 | 推荐 API | 说明 |
//! |------|----------|------|
//! | Session | `RedisCache` + 前缀 `session:` | 中间件层已用，勿在 handler 再绕过 |
//! | API Key 校验 | `get_or_set` + 短 TTL | 避免每次请求查 DB；`Option<T>` 顺带负缓存挡无效令牌 |
//! | 回源合并 | `get_or_set` | 进程内 singleflight，热点 key 失效时不打穿数据库 |
//! | 登录防爆破 | `incr` 固定窗口 | 见 `login_guard` / `RedisIpRateLimit` |
//! | Feature flag | `TwoLevelStore` | `tibba-feature` 的 `with_local_cache` opt-in；L1 对齐边界保一致性、L2 抖动防雪崩 |
//! | 分布式锁（任务去重） | `lock` | 定时任务 singleton；**只靠 TTL 释放，无 unlock**（见 `RedisCache::lock`） |
//! | 分布式锁（互斥临界区） | `lock_with_token` + `unlock` | 带属主令牌，Lua CAS 释放，只删自己的锁 |
//! | 精确限流 | `rate_limit_sliding` | Redis 滑动窗口，跨实例共享配额，无固定窗口的边界双倍效应 |
//! | 自定义原子操作 | `eval` | Lua 脚本（EVALSHA + 自动回退），见 `script` 模块 |
//! | **长阻塞**（BRPOP） | [`RedisClient::dedicated_blocking_conn`]`(max_block)` | **不归池** + **按 max_block 设置 response timeout** |
//! | 专用短写（reply loop） | [`RedisClient::dedicated_command_conn`] | 不归池，显式 5s response timeout |
//!
//! ### 池化 vs 专用阻塞连接（两件事必须一起解决）
//! 1. **不归池**：`BRPOP` 不能占 deadpool slot → `dedicated_*`
//! 2. **覆盖 redis 默认 500ms response timeout**：否则 `BRPOP` 阻塞 2s 会 `timed out`
//!    → `dedicated_blocking_conn(max_block)` 把最长阻塞做进签名，自动
//!    `response_timeout = max_block + 1s`（`max_block == 0` 则关闭超时）
//!
//! ## URI 参数（`redis://host:6379?k=v`，时长用 humantime，如 `5s` / `200ms`）
//! | 参数 | 默认 | 说明 |
//! |------|------|------|
//! | `pool_size` | `10` | 连接池大小 |
//! | `connection_timeout` | `3s` | 建连超时（池化 + 专用连接同时生效） |
//! | `wait_timeout` | `3s` | 从池中等待可用连接的超时 |
//! | `recycle_timeout` | `300ms` | 归还前健康检测（PING）超时 |
//! | `idle_timeout` | `10m` | 空闲超过即丢弃，不复用 |
//! | `max_conn_age` | `24h` | 连接最大存活时间 |
//! | `response_timeout` | `5s` | **单次命令响应超时；`0` = 不超时** |
//! | `slow` | `200ms` | 慢命令阈值，交由应用侧 `stat_callback` 判定 |
//! | `recycle_min_idle` | `1s` | 空闲不足该时长的连接跳过 PING 探活；`0` = 每次都探活 |
//!
//! ### `recycle_min_idle` 解决什么
//! deadpool 在**每次** `Pool::get()` 复用池内连接时都会调 `Manager::recycle`，
//! 本 crate 的 `RedisCache` 又是一个方法一次 `conn()`——于是每条 Redis 命令前
//! 都要先付一次 PING 往返，P50 直接翻倍。1s 内刚用过的连接断开概率极低，
//! 跳过探活基本不损失可靠性；空闲更久的仍照常探活。
//!
//! ### `response_timeout` 为什么必须可配
//! redis-rs 1.x 把默认值定为 **500ms**，且不只影响阻塞命令——大 pipeline、慢 Lua 脚本、
//! 大范围 `SCAN` 都会被它截断。deadpool-redis 自带的 Manager 不暴露该配置，因此本 crate
//! 单节点与集群都改用自建 Manager，在建连时注入 URI 值，保证**池内**连接也生效。
//!
//! ### 慢命令统计豁免阻塞命令
//! `BRPOP` 正常就要阻塞数秒，若无条件计入慢命令，会长期霸占「最慢命令」榜把真实慢查询淹掉。
//! 因此 [`is_intentional_blocking_command`] 识别出的命令（`BRPOP` / `BLPOP` / `BLMOVE` /
//! `BLMPOP` / `BZPOPMIN` / 带 `BLOCK` 的 `XREAD` 等）在**成功**时直接跳过 `stat_callback`；
//! 出错仍会上报（连接断开需要被看见），并带 `intentional_block = true` 供调用方分流。
//!
//! 默认 TTL 10 分钟；生产键名务必 `with_prefix` 隔离命名空间。

use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use std::borrow::Cow;
use std::time::Duration;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_util::parse_uri;
use validator::Validate;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("config error: {source}"))]
    Config {
        #[snafu(source(from(tibba_config::Error, Box::new)))]
        source: Box<tibba_config::Error>,
    },
    #[snafu(display("parse uri error: {source}"))]
    ParseUri {
        #[snafu(source(from(tibba_util::Error, Box::new)))]
        source: Box<tibba_util::Error>,
    },
    // 借池失败：两种模式的 Manager `Error` 同为 `RedisError`，故 PoolError 类型一致，
    // 拆两个 variant 只为在 sub_category 上区分模式
    #[snafu(display("single connect error: {source}"))]
    SingleConnect {
        source: deadpool::managed::PoolError<redis::RedisError>,
    },
    #[snafu(display("cluster connect error: {source}"))]
    ClusterConnect {
        source: deadpool::managed::PoolError<redis::RedisError>,
    },
    // 单节点 / 集群均由自建 Manager 走 `managed::Pool::builder().build()`，错误类型相同，
    // 拆两个 variant 只为在 sub_category 上区分模式
    #[snafu(display("{source}"))]
    SingleBuild {
        source: deadpool::managed::BuildError,
    },
    #[snafu(display("{source}"))]
    ClusterBuild {
        source: deadpool::managed::BuildError,
    },
    #[snafu(display("category: {category}, {source}"))]
    Redis {
        category: String,
        source: redis::RedisError,
    },
    /// URI 语义非法（如集群模式下指定了非 0 的 db 编号）。
    #[snafu(display("invalid redis uri: {message}"))]
    InvalidUri { message: String },
    #[snafu(display("{source}"))]
    Compression { source: tibba_util::Error },
    #[snafu(display("{source}"))]
    SerdeJson { source: serde_json::Error },
    #[snafu(display("category: {category}, {source}"))]
    Url {
        category: String,
        source: url::ParseError,
    },
    #[snafu(display("category: {category}, {source}"))]
    Validate {
        category: String,
        #[snafu(source(from(validator::ValidationErrors, Box::new)))]
        source: Box<validator::ValidationErrors>,
    },
}

type Result<T> = std::result::Result<T, Error>;

/// 去掉节点 URL 里的 userinfo：`scheme://user:pass@host:port` → `scheme://***@host:port`。
///
/// 按 `://` 与最后一个 `@` 切分，而不是拿密码原文去做子串替换——后者在密码恰好
/// 是 `6379` 这类常见串时会把端口一起打码，而且密码为空时完全失效。
/// 用 `rsplit_once('@')`：密码里若含未转义的 `@`，最后一个才是真正的分隔符。
fn redact_node_url(url: &str) -> Cow<'_, str> {
    let Some((scheme, rest)) = url.split_once("://") else {
        return Cow::Borrowed(url);
    };
    match rest.rsplit_once('@') {
        Some((_userinfo, host)) => Cow::Owned(format!("{scheme}://***@{host}")),
        // 无 userinfo，原样返回
        None => Cow::Borrowed(url),
    }
}

// Redis 连接配置，含校验规则
#[derive(Clone, Default, Validate)]
pub struct RedisConfig {
    // Redis 节点列表
    #[validate(length(min = 1))]
    pub nodes: Vec<String>,
    // 连接池大小
    pub pool_size: u32,
    // 建立连接的超时时间
    pub connection_timeout: Duration,
    // 等待连接的超时时间
    pub wait_timeout: Duration,
    // 回收连接时的健康检测超时时间
    pub recycle_timeout: Duration,
    // 连接空闲超时时间
    pub idle_timeout: Duration,
    // 认证密码
    pub password: Option<String>,
    // 连接最大存活时间
    pub max_conn_age: Duration,
    /// 客户端单次命令响应超时（覆盖 redis-rs 默认 500ms）。
    /// `None` = 不超时（适合需长等待的场景；池内连接也在建连时生效）。
    pub response_timeout: Option<Duration>,
    /// 慢命令统计阈值：超过则由应用侧 `stat_callback` 记为 slow（阻塞命令会豁免）。
    pub slow_cmd_threshold: Duration,
    /// 复用连接时，空闲时长小于该值就跳过 PING 探活（`0` = 每次都探活）。
    pub recycle_min_idle: Duration,
}

/// 手写 `Debug` 而非 derive：本结构有**两处**都带着 Redis 口令。
///
/// - `password` 是明文口令本身；
/// - `nodes` 存的是拼好 auth 的完整节点 URL（`redis://:secret@host:6379`），
///   同样含口令——这一处最容易被漏掉。
///
/// derive 出来的 `{:?}` 会把两者原样打进日志或 panic 回溯，而连接失败时正是最
/// 想打印配置的时候。这里只保留「是否配了口令」这一位信息，值一律不出现。
/// 同 `tibba_config::Config` 的处理。
impl std::fmt::Debug for RedisConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let nodes: Vec<Cow<'_, str>> = self.nodes.iter().map(|n| redact_node_url(n)).collect();
        f.debug_struct("RedisConfig")
            .field("nodes", &nodes)
            .field("pool_size", &self.pool_size)
            .field("connection_timeout", &self.connection_timeout)
            .field("wait_timeout", &self.wait_timeout)
            .field("recycle_timeout", &self.recycle_timeout)
            .field("idle_timeout", &self.idle_timeout)
            // 只暴露「配没配」，不暴露值
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("max_conn_age", &self.max_conn_age)
            .field("response_timeout", &self.response_timeout)
            .field("slow_cmd_threshold", &self.slow_cmd_threshold)
            .field("recycle_min_idle", &self.recycle_min_idle)
            .finish()
    }
}

fn default_pool_size() -> u32 {
    10
}

/// 池化连接默认 response timeout：5s。
/// redis-rs 默认仅 500ms，大 pipeline / 慢 Lua / SCAN 易误超时；URI 可覆盖。
const DEFAULT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// 默认慢命令阈值（与常见 `slow=200ms` 运维约定一致；URI `slow=` 可覆盖）。
const DEFAULT_SLOW_CMD_THRESHOLD: Duration = Duration::from_millis(200);
/// 默认「跳过探活」的空闲阈值：1s。
///
/// deadpool 每次从池里取连接都会调 `Manager::recycle`（对本 crate 而言就是每条
/// Redis 命令一次 PING 往返）。1s 内刚成功用过的连接几乎不可能已断开——服务端
/// `timeout`、keepalive、网络中断的时间尺度都远大于此——这次探活买不到信息，
/// 却要实打实多付一个 RTT。空闲更久的连接照常探活。
const DEFAULT_RECYCLE_MIN_IDLE: Duration = Duration::from_secs(1);

#[derive(Deserialize, Debug, Clone)]
struct RedisParams {
    #[serde(default = "default_pool_size")]
    pool_size: u32,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    connection_timeout: Option<Duration>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    wait_timeout: Option<Duration>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    recycle_timeout: Option<Duration>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    max_conn_age: Option<Duration>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    idle_timeout: Option<Duration>,
    /// 客户端 response timeout。省略 → 5s；`0` → 不超时（None）。
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    response_timeout: Option<Duration>,
    /// 慢命令阈值，对应 URI `slow=200ms`。省略 → 200ms。
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    #[serde(alias = "slow")]
    slow: Option<Duration>,
    /// 跳过 PING 探活的空闲阈值。省略 → 1s；`0` → 每次复用都探活。
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    recycle_min_idle: Option<Duration>,
    password: Option<String>,
}

// 从配置中解析并构建 RedisConfig
fn new_redis_config(config: &Config) -> Result<RedisConfig> {
    let uri = config.get_string("uri").context(ConfigSnafu)?;
    let parsed = parse_uri::<RedisParams>(&uri).context(ParseUriSnafu)?;
    // 保留原始 scheme（如 `rediss://` 表示 TLS）；之前硬编码 `redis://` 会
    // 让 TLS 配置被静默降级为明文，且无任何错误或警告
    let scheme = parsed.schema;
    // userinfo 里的密码必须拼回每个节点 URL：host_strings() 只输出 host:port，
    // 把 userinfo 剥掉了。少了这一步，redis-rs 的 Client::open / ClusterClient
    // 拿到的是无 auth 的 URL，带密码的实例会 AUTH 失败（cluster 报 NOAUTH）。
    let userinfo_password = parsed.password;
    let auth = match (parsed.username, userinfo_password) {
        (Some(u), Some(p)) => format!("{u}:{p}@"),
        (None, Some(p)) => format!(":{p}@"),
        (Some(u), None) => format!("{u}@"),
        (None, None) => String::new(),
    };
    // URI 的 path 段是 **db 编号**（`redis://host:6379/3`）。`host_strings()` 只输出
    // `host:port`，此前直接丢掉了 path——配了 db 3 的部署会静默连到 db 0，读写全落
    // 在错误的库上而没有任何提示。与之前「硬编码 redis:// 导致 TLS 静默降级」同类。
    let db = parsed.path.filter(|p| !p.is_empty());
    let hosts = parsed.host_strings();
    // Redis Cluster 只有 db 0；与其让连接建起来之后行为诡异，不如启动期直接拒绝
    if hosts.len() > 1
        && let Some(db) = db
        && db != "0"
    {
        return Err(Error::InvalidUri {
            message: format!("redis cluster does not support db index (got {db:?})"),
        });
    }
    let db_suffix = db.map(|d| format!("/{d}")).unwrap_or_default();
    let nodes = hosts
        .iter()
        .map(|item| format!("{scheme}://{auth}{item}{db_suffix}"))
        .collect();
    let query = parsed.query;
    // 密码优先取 userinfo（redis://:pw@host），回退到查询串 ?password=。
    // 连接本身已从上面拼好的 URL 取到 auth；这里保留一份供 pool 日志打码。
    let password = userinfo_password.map(str::to_string).or(query.password);
    // response_timeout：未配置 → 5s；显式 0 → None（关闭）；其它 → 该值
    let response_timeout = match query.response_timeout {
        None => Some(DEFAULT_RESPONSE_TIMEOUT),
        Some(d) if d.is_zero() => None,
        Some(d) => Some(d),
    };
    let redis_config = RedisConfig {
        nodes,
        pool_size: query.pool_size,
        connection_timeout: query.connection_timeout.unwrap_or(Duration::from_secs(3)),
        wait_timeout: query.wait_timeout.unwrap_or(Duration::from_secs(3)),
        // 检测请求是否可用的超时时间，默认300ms
        recycle_timeout: query.recycle_timeout.unwrap_or(Duration::from_millis(300)),
        max_conn_age: query.max_conn_age.unwrap_or(Duration::from_secs(24 * 3600)),
        // 由于pool本身没有idle timeout处理，因此现在的模块在复用前判断，需要根据redis server设置调整，默认10分钟
        idle_timeout: query.idle_timeout.unwrap_or(Duration::from_secs(10 * 60)),
        password,
        response_timeout,
        slow_cmd_threshold: query
            .slow
            .filter(|d| !d.is_zero())
            .unwrap_or(DEFAULT_SLOW_CMD_THRESHOLD),
        // 显式 0 表示「每次都探活」，故与 slow 不同，这里不过滤零值
        recycle_min_idle: query.recycle_min_idle.unwrap_or(DEFAULT_RECYCLE_MIN_IDLE),
    };
    redis_config
        .validate()
        .context(ValidateSnafu { category: "redis" })?;
    Ok(redis_config)
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        // 基础设施错误（Redis 不可达等）→ 500 + 异常标记
        fn infra(err: BaseError) -> BaseError {
            err.with_status(500).with_exception(true)
        }
        let err = match val {
            Error::Config { source } => BaseError::new(*source).with_sub_category("config"),
            Error::ParseUri { source } => BaseError::new(*source).with_sub_category("parse_uri"),
            Error::SingleConnect { source } => {
                infra(BaseError::new(source).with_sub_category("single_connect"))
            }
            Error::ClusterConnect { source } => {
                infra(BaseError::new(source).with_sub_category("cluster_connect"))
            }
            Error::SingleBuild { source } => {
                infra(BaseError::new(source).with_sub_category("single_build"))
            }
            Error::ClusterBuild { source } => {
                infra(BaseError::new(source).with_sub_category("cluster_build"))
            }
            Error::Redis { category, source } => {
                infra(BaseError::new(source).with_sub_category(&category))
            }
            // 部署配置错误，启动期就该被发现
            Error::InvalidUri { message } => BaseError::new(message)
                .with_sub_category("invalid_uri")
                .with_status(500)
                .with_exception(true),
            Error::Compression { source } => BaseError::new(source)
                .with_sub_category("compression")
                .with_exception(true),
            Error::SerdeJson { source } => BaseError::new(source)
                .with_sub_category("serde_json")
                .with_exception(true),
            Error::Url { category, source } => {
                infra(BaseError::new(source).with_sub_category(&category))
            }
            Error::Validate { category, source } => {
                BaseError::new(*source).with_sub_category(&category)
            }
        };
        err.with_category("cache")
    }
}

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:cache=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:cache";

/// 重导出 `redis`，调用方可直接 `tibba_cache::redis::cmd(...)`，
/// 既省去自行声明 redis 依赖，也避免版本不一致导致 trait 不通用。
pub use redis;

mod cache;
mod invalidation;
mod pool;
mod script;
mod single_flight;
mod ttl_fifo_store;
mod two_level_store;

pub use cache::*;
pub use invalidation::*;
pub use pool::*;
pub use script::*;
pub use ttl_fifo_store::*;
pub use two_level_store::*;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn config_with_uri(uri: &str) -> Config {
        Config::builder()
            .add_toml(format!("uri = \"{uri}\""))
            .build()
            .expect("构造测试配置")
    }

    /// IPv6 部署的端到端守卫：URI 里的 `[::1]` 必须一路带着方括号拼进节点 URL。
    ///
    /// 此前 `parse_uri` 把 `[::1]` 拆成 host=`[:` / port=`1]`，解析直接失败，
    /// IPv6 环境下应用起不来。
    #[test]
    fn ipv6_redis_uri_produces_bracketed_node_urls() {
        let config = config_with_uri("redis://[::1]:6379");
        let redis_config = new_redis_config(&config).expect("IPv6 URI 必须能解析");
        assert_eq!(redis_config.nodes, vec!["redis://[::1]:6379".to_string()]);
    }

    /// IPv6 + 密码 + 集群多节点：auth 与方括号都要正确拼回每个节点。
    #[test]
    fn ipv6_cluster_uri_keeps_auth_and_brackets() {
        let config = config_with_uri("redis://:secret@[::1]:6379,[fe80::2]:6380");
        let redis_config = new_redis_config(&config).unwrap();
        assert_eq!(
            redis_config.nodes,
            vec![
                "redis://:secret@[::1]:6379".to_string(),
                "redis://:secret@[fe80::2]:6380".to_string(),
            ]
        );
        assert_eq!(redis_config.password.as_deref(), Some("secret"));
    }

    /// **回归守卫**：URI 里的 db 编号必须拼回节点 URL。
    ///
    /// 旧实现只取 `host_strings()`（仅 `host:port`），path 段被丢掉——
    /// `redis://host:6379/3` 会静默连到 db 0，全部读写落在错误的库上。
    #[test]
    fn db_index_is_preserved_in_node_url() {
        let config = config_with_uri("redis://127.0.0.1:6379/3");
        let redis_config = new_redis_config(&config).unwrap();
        assert_eq!(
            redis_config.nodes,
            vec!["redis://127.0.0.1:6379/3".to_string()]
        );

        // 带 auth 与 IPv6 时同样要带上
        let config = config_with_uri("redis://:pw@[::1]:6379/2");
        let redis_config = new_redis_config(&config).unwrap();
        assert_eq!(
            redis_config.nodes,
            vec!["redis://:pw@[::1]:6379/2".to_string()]
        );

        // 未指定 db 时不得凭空加斜杠
        let config = config_with_uri("redis://127.0.0.1:6379");
        let redis_config = new_redis_config(&config).unwrap();
        assert_eq!(
            redis_config.nodes,
            vec!["redis://127.0.0.1:6379".to_string()]
        );
    }

    /// 集群不支持非 0 的 db，应在启动期报错而不是连上之后行为诡异。
    #[test]
    fn cluster_rejects_non_zero_db_index() {
        let config = config_with_uri("redis://host1:6379,host2:6380/3");
        let err = new_redis_config(&config).expect_err("集群 + db 3 应当被拒绝");
        assert!(
            err.to_string().contains("does not support db index"),
            "错误信息应说明原因，实际: {err}"
        );

        // db 0 等价于不指定，应放行
        let config = config_with_uri("redis://host1:6379,host2:6380/0");
        assert!(new_redis_config(&config).is_ok());
    }

    /// 口令含未转义 `@` 时，节点 URL 与打码都要正确（依赖 parse_uri 的 rsplit 修复）。
    #[test]
    fn password_with_at_sign_yields_correct_node_url() {
        let config = config_with_uri("redis://user:p@ss@host:6379");
        let redis_config = new_redis_config(&config).unwrap();
        assert_eq!(
            redis_config.nodes,
            vec!["redis://user:p@ss@host:6379".to_string()]
        );
        assert_eq!(redis_config.password.as_deref(), Some("p@ss"));
        let debug = format!("{redis_config:?}");
        assert!(!debug.contains("p@ss"), "Debug 输出泄漏了口令: {debug}");
    }

    /// IPv4 路径不能被 IPv6 支持改坏。
    #[test]
    fn ipv4_uri_is_unaffected() {
        let config = config_with_uri("redis://127.0.0.1:6379");
        let redis_config = new_redis_config(&config).unwrap();
        assert_eq!(
            redis_config.nodes,
            vec!["redis://127.0.0.1:6379".to_string()]
        );
    }

    #[test]
    fn redact_node_url_strips_userinfo_only() {
        assert_eq!(
            redact_node_url("redis://:secret@host:6379"),
            "redis://***@host:6379"
        );
        assert_eq!(
            redact_node_url("redis://user:secret@host:6379"),
            "redis://***@host:6379"
        );
        // 无 userinfo：原样返回，不应凭空加 ***
        assert_eq!(redact_node_url("redis://host:6379"), "redis://host:6379");
        // IPv6 节点
        assert_eq!(
            redact_node_url("redis://:pw@[::1]:6379"),
            "redis://***@[::1]:6379"
        );
        // 密码含未转义 `@`：取最后一个 `@` 作分隔符
        assert_eq!(
            redact_node_url("redis://user:p@ss@host:6379"),
            "redis://***@host:6379"
        );
        // 非 URL 形态不处理
        assert_eq!(redact_node_url("not-a-url"), "not-a-url");
    }

    /// `RedisConfig` 的 `{:?}` 绝不能吐出口令——它藏在**两处**：
    /// `password` 字段本身，以及 `nodes` 里拼好 auth 的完整节点 URL。
    /// 回归到 `derive(Debug)` 时本例会失败。
    #[test]
    fn debug_leaks_neither_password_field_nor_node_urls() {
        let config = config_with_uri("redis://:sup3r-s3cret@host1:6379,host2:6380");
        let redis_config = new_redis_config(&config).unwrap();

        // 前提：口令确实同时存在于两处，否则本测试是空转
        assert_eq!(redis_config.password.as_deref(), Some("sup3r-s3cret"));
        assert!(
            redis_config
                .nodes
                .iter()
                .any(|n| n.contains("sup3r-s3cret"))
        );

        let debug = format!("{redis_config:?}");
        assert!(
            !debug.contains("sup3r-s3cret"),
            "Debug 输出泄漏了口令: {debug}"
        );
        // 结构信息仍需可见，否则排查连接问题时 Debug 就没用了
        assert!(debug.contains("host1:6379"));
        assert!(debug.contains("host2:6380"));
        assert!(
            debug.contains("<redacted>"),
            "应能看出「配了口令」这一位信息"
        );

        // 未配口令时不应显示 <redacted>，避免误导
        let plain = new_redis_config(&config_with_uri("redis://host:6379")).unwrap();
        let debug = format!("{plain:?}");
        assert!(!debug.contains("<redacted>"));
        assert!(debug.contains("None"));
    }
}
