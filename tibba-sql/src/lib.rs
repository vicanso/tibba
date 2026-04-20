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

/// Tracing target for all log events in this crate.
/// Use `RUST_LOG=tibba:sql=info` (or `debug`) to filter these logs.
const LOG_TARGET: &str = "tibba:sql";

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("sqlx error: {source}"))]
    Sqlx {
        #[snafu(source(from(sqlx::Error, Box::new)))]
        source: Box<sqlx::Error>,
    },

    #[snafu(display("validate error: {source}"))]
    Validate {
        #[snafu(source(from(validator::ValidationErrors, Box::new)))]
        source: Box<validator::ValidationErrors>,
    },

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

#[derive(Debug, Clone, Default, Validate)]
pub struct DatabaseConfig {
    pub origin_url: String,
    #[validate(length(min = 10))]
    pub url: String,
    #[validate(range(min = 2, max = 1000))]
    pub max_connections: u32,
    #[validate(range(min = 0, max = 10))]
    pub min_connections: u32,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
    pub test_before_acquire: bool,
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

/// Tracks per-pool connection and execution statistics.
///
/// All counters are atomically reset to zero on each call to [`PoolStat::stat`].
#[derive(Debug, Default)]
pub struct PoolStat {
    /// Number of new connections established since the last stat read.
    connected: AtomicU32,
    /// Number of connection acquisitions since the last stat read.
    executions: AtomicUsize,
    /// Accumulated idle time (seconds) across all acquisitions since the last stat read.
    idle_for: AtomicU64,
}

impl PoolStat {
    /// Atomically reads and resets all counters.
    ///
    /// Returns `(connected, executions, idle_for_secs)`.
    pub fn stat(&self) -> (u32, usize, u64) {
        let connected = self.connected.swap(0, Ordering::Relaxed);
        let executions = self.executions.swap(0, Ordering::Relaxed);
        let idle_for = self.idle_for.swap(0, Ordering::Relaxed);
        (connected, executions, idle_for)
    }
}

/// Parses and validates a `DatabaseConfig` from the application config.
fn new_database_config(config: &Config) -> Result<DatabaseConfig> {
    let origin_url = config.get_string("uri").context(ConfigSnafu)?;
    // `ParsedUri` borrows the string, so clone before moving `origin_url` into the struct.
    let url_str = origin_url.clone();
    let parsed = parse_uri::<DatabaseQuery>(&url_str).context(ParseUriSnafu)?;

    let mut url = parsed.url().context(ParseUriSnafu)?;
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

/// Creates and connects a PostgreSQL connection pool.
///
/// When `pool_stat` is provided, connection and acquisition events are tracked.
pub async fn new_pg_pool(config: &Config, pool_stat: Option<Arc<PoolStat>>) -> Result<PgPool> {
    let database_config = new_database_config(config)?;
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
