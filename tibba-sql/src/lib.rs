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

use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_util::parse_uri;
use tracing::info;
use validator::Validate;

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:sql=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:sql";

#[derive(Debug, Snafu)]
pub enum Error {
    /// SQLx 数据库操作错误，属于基础设施异常。
    #[snafu(display("sqlx error: {source}"))]
    Sqlx {
        #[snafu(source(from(sqlx::Error, Box::new)))]
        source: Box<sqlx::Error>,
    },
    /// 配置字段校验失败（如连接数范围越界等）。
    #[snafu(display("validate error: {source}"))]
    Validate {
        #[snafu(source(from(validator::ValidationErrors, Box::new)))]
        source: Box<validator::ValidationErrors>,
    },
    /// 读取应用配置失败。
    #[snafu(display("config error: {source}"))]
    Config {
        #[snafu(source(from(tibba_config::Error, Box::new)))]
        source: Box<tibba_config::Error>,
    },
    /// 数据库 URI 解析失败。
    #[snafu(display("parse uri error: {source}"))]
    ParseUri {
        #[snafu(source(from(tibba_util::Error, Box::new)))]
        source: Box<tibba_util::Error>,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Sqlx { source } => BaseError::new(source)
                .with_sub_category("sqlx")
                .with_exception(true),
            Error::Validate { source } => BaseError::new(source).with_sub_category("validate"),
            Error::Config { source } => BaseError::new(source).with_sub_category("config"),
            Error::ParseUri { source } => BaseError::new(source).with_sub_category("parse_uri"),
        };
        err.with_category("sql")
    }
}

type Result<T> = std::result::Result<T, Error>;

/// 数据库连接池配置，字段均通过 URI 查询参数解析后填充。
#[derive(Debug, Clone, Default, Validate)]
pub struct DatabaseConfig {
    /// 原始数据库 URI（含密码，仅内部使用）
    pub origin_url: String,
    /// 脱敏后的数据库连接 URL（去除查询参数）
    #[validate(length(min = 10))]
    pub url: String,
    /// 连接池最大连接数（2–1000）
    #[validate(range(min = 2, max = 1000))]
    pub max_connections: u32,
    /// 连接池最小保活连接数（0–10）
    #[validate(range(min = 0, max = 10))]
    pub min_connections: u32,
    /// 建立连接的超时时间
    pub connect_timeout: Duration,
    /// 连接空闲超时时间，超出后连接将被回收
    pub idle_timeout: Duration,
    /// 连接最大存活时间，超出后强制重建
    pub max_lifetime: Duration,
    /// 取出连接前是否先执行健康检测
    pub test_before_acquire: bool,
    /// 数据库密码（从 URI 中提取，用于日志脱敏）
    pub password: Option<String>,
}

fn default_max_connections() -> u32 {
    10
}
fn default_min_connections() -> u32 {
    2
}
fn default_connect_timeout() -> Duration {
    Duration::from_secs(3)
}
fn default_idle_timeout() -> Duration {
    Duration::from_secs(60)
}
fn default_max_lifetime() -> Duration {
    Duration::from_secs(6 * 60 * 60)
}
fn default_test_before_acquire() -> bool {
    true
}

/// 从 URI 查询字符串反序列化的连接池参数，未设置时使用各自的默认值。
#[derive(Deserialize, Debug, Clone)]
struct DatabaseQuery {
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_connect_timeout")]
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Duration,
    #[serde(default = "default_idle_timeout")]
    #[serde(with = "humantime_serde")]
    pub idle_timeout: Duration,
    #[serde(default = "default_max_lifetime")]
    #[serde(with = "humantime_serde")]
    pub max_lifetime: Duration,
    #[serde(default = "default_test_before_acquire")]
    pub test_before_acquire: bool,
}

/// 连接池运行时统计，所有计数器在每次调用 `stat()` 时原子性地读取并重置为 0。
#[derive(Debug, Default)]
pub struct PoolStat {
    /// 自上次读取以来新建的连接数
    connected: AtomicU32,
    /// 自上次读取以来连接被取出（acquire）的次数
    executions: AtomicUsize,
    /// 自上次读取以来所有连接取出前累计的空闲时间（秒）
    idle_for: AtomicU64,
}

impl PoolStat {
    /// 原子性地读取并重置所有计数器，返回 `(新建连接数, 取出次数, 累计空闲秒数)`。
    pub fn stat(&self) -> (u32, usize, u64) {
        let connected = self.connected.swap(0, Ordering::Relaxed);
        let executions = self.executions.swap(0, Ordering::Relaxed);
        let idle_for = self.idle_for.swap(0, Ordering::Relaxed);
        (connected, executions, idle_for)
    }
}

/// 从应用配置中解析并校验 `DatabaseConfig`。
/// 密码从 URI 中单独提取，用于后续日志脱敏；URL 去除查询参数后存储。
fn new_database_config(config: &Config) -> Result<DatabaseConfig> {
    let origin_url = config.get_string("uri").context(ConfigSnafu)?;
    // ParsedUri 借用字符串，需提前 clone 再将 origin_url 移入结构体
    let url_str = origin_url.clone();
    let parsed = parse_uri::<DatabaseQuery>(&url_str).context(ParseUriSnafu)?;

    let mut url = parsed.url().context(ParseUriSnafu)?;
    // 去除查询参数，避免将连接池配置混入实际连接 URL
    url.set_query(None);

    let query = &parsed.query;
    let database_config = DatabaseConfig {
        url: url.to_string(),
        origin_url,
        max_connections: query.max_connections,
        min_connections: query.min_connections,
        connect_timeout: query.connect_timeout,
        idle_timeout: query.idle_timeout,
        max_lifetime: query.max_lifetime,
        test_before_acquire: query.test_before_acquire,
        password: parsed.password.map(|v| v.to_string()),
    };
    database_config.validate().context(ValidateSnafu)?;
    Ok(database_config)
}

/// 根据配置创建并连接 PostgreSQL 连接池。
/// 若提供了 `pool_stat`，则通过 `after_connect` 和 `before_acquire` 钩子
/// 原子性地记录新建连接数、取出次数和连接空闲时间。
pub async fn new_pg_pool(config: &Config, pool_stat: Option<Arc<PoolStat>>) -> Result<PgPool> {
    let database_config = new_database_config(config)?;
    // 日志中脱敏密码
    let password = database_config.password.clone().unwrap_or_default();
    let url = database_config.url.replace(&password, "***");
    info!(target: LOG_TARGET, url, "connect to database");

    let mut options = PgPoolOptions::new()
        .max_connections(database_config.max_connections)
        .min_connections(database_config.min_connections)
        .idle_timeout(database_config.idle_timeout)
        .max_lifetime(database_config.max_lifetime)
        .test_before_acquire(database_config.test_before_acquire);

    if let Some(pool_stat) = pool_stat {
        let after_connect_pool_stat = pool_stat.clone();
        let before_acquire_pool_stat = pool_stat.clone();
        options = options
            .after_connect(move |_conn, _meta| {
                let stat = after_connect_pool_stat.clone();
                Box::pin(async move {
                    // 新建连接后累加计数并打印日志
                    let connected = stat.connected.fetch_add(1, Ordering::Relaxed) + 1;
                    info!(
                        target: LOG_TARGET,
                        connected,
                        "after connect"
                    );
                    Ok(())
                })
            })
            .before_acquire(move |_conn, meta| {
                let stat = before_acquire_pool_stat.clone();
                Box::pin(async move {
                    // 取出连接前记录空闲时间和连接年龄，便于监控连接复用情况
                    let idle = meta.idle_for.as_secs();
                    info!(
                        target: LOG_TARGET,
                        age = meta.age.as_secs(),
                        idle,
                        "before acquire"
                    );
                    stat.executions.fetch_add(1, Ordering::Relaxed);
                    stat.idle_for.fetch_add(idle, Ordering::Relaxed);
                    Ok(true)
                })
            });
    }

    options
        .connect(database_config.url.as_str())
        .await
        .context(SqlxSnafu)
}
