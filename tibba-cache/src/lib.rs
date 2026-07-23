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
//! | API Key 校验 | `get_struct` / 短 TTL | 避免每次请求查 DB |
//! | 登录防爆破 | `incr` 固定窗口 | 见 `login_guard` / `RedisIpRateLimit` |
//! | Feature flag | 进程内 + Redis 双层 | `two_level_store` |
//! | 分布式锁 | `lock` | 定时任务 singleton |
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

// Redis 连接配置，含校验规则
#[derive(Debug, Clone, Default, Validate)]
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
}

fn default_pool_size() -> u32 {
    10
}

/// 池化连接默认 response timeout：5s。
/// redis-rs 默认仅 500ms，大 pipeline / 慢 Lua / SCAN 易误超时；URI 可覆盖。
const DEFAULT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// 默认慢命令阈值（与常见 `slow=200ms` 运维约定一致；URI `slow=` 可覆盖）。
const DEFAULT_SLOW_CMD_THRESHOLD: Duration = Duration::from_millis(200);

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
    let nodes = parsed
        .host_strings()
        .iter()
        .map(|item| format!("{scheme}://{auth}{item}"))
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
mod pool;
mod ttl_lru_store;
mod two_level_store;

pub use cache::*;
pub use pool::*;
pub use ttl_lru_store::*;
pub use two_level_store::*;
