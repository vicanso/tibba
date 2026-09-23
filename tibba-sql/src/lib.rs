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

use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_util::parse_uri;
use tracing::{debug, info};
use url::Url;
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
///
/// `url` 含明文密码，因此不派生 `Debug`：手写实现用 `redacted_url` 顶替，
/// 避免凭据随 `{:?}` 进入日志或错误信息。
#[derive(Clone, Default, Validate)]
#[validate(schema(function = "validate_pool_bounds"))]
pub struct DatabaseConfig {
    /// 实际用于建连的 URL（含密码，已去除连接池查询参数）
    #[validate(length(min = 10))]
    pub url: String,
    /// 密码已替换为 [`PASSWORD_MASK`] 的 URL，仅用于日志输出
    pub redacted_url: String,
    /// 连接池最大连接数（2–1000）
    #[validate(range(min = 2, max = 1000))]
    pub max_connections: u32,
    /// 连接池最小保活连接数（0–10）
    #[validate(range(min = 0, max = 10))]
    pub min_connections: u32,
    /// 取得连接的超时时间（含排队等空闲连接与新建连接），映射到 sqlx 的
    /// `acquire_timeout`。此前解析了却从未应用，实际生效的是 sqlx 默认的 30s。
    pub connect_timeout: Duration,
    /// 连接空闲超时时间，超出后连接将被回收
    pub idle_timeout: Duration,
    /// 连接最大存活时间，超出后强制重建
    pub max_lifetime: Duration,
    /// 取出连接前是否先执行健康检测
    pub test_before_acquire: bool,
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseConfig")
            // 用脱敏 URL 顶替含密码的 `url` 字段
            .field("url", &self.redacted_url)
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout", &self.connect_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .field("max_lifetime", &self.max_lifetime)
            .field("test_before_acquire", &self.test_before_acquire)
            .finish()
    }
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

/// 本 crate 自己消费的连接池参数名；建连前只剥离这些，其余原样交给 sqlx。
const POOL_PARAMS: &[&str] = &[
    "max_connections",
    "min_connections",
    "connect_timeout",
    "idle_timeout",
    "max_lifetime",
    "test_before_acquire",
];

/// 从 URL 查询串里**只**剥离连接池参数，保留 Postgres 自身的连接参数。
///
/// 此前是 `url.set_query(None)`——整个查询串一起丢掉，`sslmode=require`、
/// `sslrootcert`、`application_name`、`options` 全部被静默删除。配置了
/// `sslmode=require` 的部署实际以 sqlx 默认的 `prefer` 建连：不校验证书，服务端
/// 不支持 TLS 时直接退回明文，而且没有任何报错或告警。与此前 Redis 的
/// `rediss://` 被硬编码成 `redis://` 是同一类静默降级。
fn strip_pool_params(url: &mut Url) {
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !POOL_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
}

/// 最小连接数不得超过最大连接数：否则连接池的保活目标永远达不到，
/// sqlx 会在后台不停尝试补齐连接。
fn validate_pool_bounds(
    config: &DatabaseConfig,
) -> std::result::Result<(), validator::ValidationError> {
    if config.min_connections > config.max_connections {
        return Err(validator::ValidationError::new(
            "min_connections must not exceed max_connections",
        ));
    }
    Ok(())
}

/// 脱敏 URL 中替换真实密码的占位符。
const PASSWORD_MASK: &str = "***";

/// 生成日志用的脱敏 URL：有密码则替换为 [`PASSWORD_MASK`]，无密码原样返回。
///
/// 按 URL 结构替换而非 `str::replace`：后者是子串匹配，密码若恰好是 `5432`
/// 或主机名的一部分，会把端口 / 主机一起改掉。
fn redact_url(mut url: Url) -> String {
    if url.password().is_some() && url.set_password(Some(PASSWORD_MASK)).is_err() {
        // 带密码的 URL 必然有 host，理论上不会失败；兜底也绝不回退到明文
        return String::from("<unprintable database url>");
    }
    url.to_string()
}

/// 从应用配置中解析并校验 `DatabaseConfig`。
/// URL 去除连接池查询参数后用于建连，同时生成一份脱敏副本供日志使用。
fn new_database_config(config: &Config) -> Result<DatabaseConfig> {
    let origin_url = config.get_string("uri").context(ConfigSnafu)?;
    let parsed = parse_uri::<DatabaseQuery>(&origin_url).context(ParseUriSnafu)?;

    let mut url = parsed.url().context(ParseUriSnafu)?;
    // 只剥离连接池参数；sslmode 等 Postgres 连接参数必须保留，见 strip_pool_params
    strip_pool_params(&mut url);

    let query = &parsed.query;
    let database_config = DatabaseConfig {
        url: url.to_string(),
        redacted_url: redact_url(url),
        max_connections: query.max_connections,
        min_connections: query.min_connections,
        connect_timeout: query.connect_timeout,
        idle_timeout: query.idle_timeout,
        max_lifetime: query.max_lifetime,
        test_before_acquire: query.test_before_acquire,
    };
    database_config.validate().context(ValidateSnafu)?;
    Ok(database_config)
}

/// 根据配置创建并连接 PostgreSQL 连接池。
/// 若提供了 `pool_stat`，则通过 `after_connect` 和 `before_acquire` 钩子
/// 原子性地记录新建连接数、取出次数和连接空闲时间。
pub async fn new_pg_pool(config: &Config, pool_stat: Option<Arc<PoolStat>>) -> Result<PgPool> {
    let database_config = new_database_config(config)?;
    info!(
        target: LOG_TARGET,
        url = database_config.redacted_url,
        "connect to database"
    );

    let mut options = PgPoolOptions::new()
        .max_connections(database_config.max_connections)
        .min_connections(database_config.min_connections)
        .acquire_timeout(database_config.connect_timeout)
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
                    // 取出连接前记录空闲时间和连接年龄，便于监控连接复用情况。
                    // 每次取连接（≈ 每条查询）都会触发，用 debug 级别避免生产环境刷屏
                    let idle = meta.idle_for.as_secs();
                    debug!(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_replaces_only_the_password_component() {
        // 密码与端口同为 5432：子串替换会连端口一起改掉，按结构替换不会
        let url = Url::parse("postgres://app:5432@db.internal:5432/tibba").expect("valid url");
        assert_eq!(redact_url(url), "postgres://app:***@db.internal:5432/tibba");
    }

    #[test]
    fn redact_url_keeps_password_free_url_unchanged() {
        let raw = "postgres://app@db.internal:5432/tibba";
        let url = Url::parse(raw).expect("valid url");
        assert_eq!(redact_url(url), raw);
    }

    fn config(uri: &str) -> Config {
        Config::builder()
            .add_toml(format!("uri = \"{uri}\""))
            .build()
            .expect("构造测试配置")
    }

    /// **回归守卫**：Postgres 自身的连接参数（尤其 sslmode）必须保留到建连 URL。
    ///
    /// 此前 `set_query(None)` 把它们和连接池参数一起丢掉，`sslmode=require`
    /// 被静默降级成 sqlx 默认的 `prefer`。
    #[test]
    fn postgres_connection_params_survive_pool_param_stripping() {
        let cfg = new_database_config(&config(
            "postgres://app:pw@db:5432/tibba?sslmode=require&max_connections=20&application_name=tibba&idle_timeout=30s",
        ))
        .expect("解析配置");
        let url = Url::parse(&cfg.url).expect("合法 URL");
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert!(
            pairs.contains(&("sslmode".into(), "require".into())),
            "{pairs:?}"
        );
        assert!(
            pairs.contains(&("application_name".into(), "tibba".into())),
            "{pairs:?}"
        );
        // 连接池参数不得混进建连 URL
        assert!(
            pairs
                .iter()
                .all(|(k, _)| !POOL_PARAMS.contains(&k.as_str())),
            "{pairs:?}"
        );
        // 但仍被正确解析
        assert_eq!(cfg.max_connections, 20);
        assert_eq!(cfg.idle_timeout, Duration::from_secs(30));
    }

    #[test]
    fn url_without_extra_params_has_no_query() {
        let cfg = new_database_config(&config("postgres://app:pw@db:5432/tibba?max_connections=5"))
            .expect("解析配置");
        assert_eq!(cfg.url, "postgres://app:pw@db:5432/tibba");
    }

    #[test]
    fn min_connections_above_max_is_rejected() {
        let err = new_database_config(&config(
            "postgres://app:pw@db:5432/tibba?max_connections=2&min_connections=5",
        ))
        .expect_err("min > max 应当被拒绝");
        assert!(matches!(err, Error::Validate { .. }), "{err}");
    }

    #[test]
    fn debug_output_never_contains_the_password() {
        let config = DatabaseConfig {
            url: "postgres://app:s3cret@db.internal:5432/tibba".into(),
            redacted_url: "postgres://app:***@db.internal:5432/tibba".into(),
            ..Default::default()
        };
        let debug = format!("{config:?}");
        assert!(
            !debug.contains("s3cret"),
            "密码不得出现在 Debug 输出中: {debug}"
        );
        assert!(debug.contains("***"), "应输出脱敏 URL: {debug}");
    }
}
