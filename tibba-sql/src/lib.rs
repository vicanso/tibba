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
use snafu::Snafu;
use sqlx::MySqlPool;
use sqlx::pool::PoolOptions;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_util::parse_uri;
use tracing::info;
use validator::Validate;

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

#[derive(Deserialize, Debug, Clone)]
struct DatabaseQuery {
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Option<Duration>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub idle_timeout: Option<Duration>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub max_lifetime: Option<Duration>,
    pub test_before_acquire: Option<bool>,
}

// Creates a new DatabaseConfig instance from the configuration
fn new_database_config(config: &Config) -> Result<DatabaseConfig> {
    let origin_url = config.get_str("uri", "");
    if origin_url.is_empty() {
        return Err(Error::Common {
            category: "config".to_string(),
            message: "uri is empty".to_string(),
        });
    }
    let url = origin_url.clone();
    let parsed = parse_uri::<DatabaseQuery>(&url).map_err(|e| Error::Common {
        category: "config".to_string(),
        message: e.to_string(),
    })?;

    let mut url = parsed.url().map_err(|e| Error::Common {
        category: "config".to_string(),
        message: e.to_string(),
    })?;
    url.set_query(None);

    let query = &parsed.query;
    let database_config = DatabaseConfig {
        origin_url,
        url: url.to_string(),
        max_connections: query.max_connections,
        min_connections: query.min_connections,
        connect_timeout: query.connect_timeout.unwrap_or(Duration::from_secs(3)),
        idle_timeout: query.idle_timeout.unwrap_or(Duration::from_secs(60)),
        max_lifetime: query
            .max_lifetime
            .unwrap_or(Duration::from_secs(6 * 60 * 60)),
        test_before_acquire: query.test_before_acquire.unwrap_or(true),
        password: parsed.password.map(|v| v.to_string()),
    };
    database_config
        .validate()
        .map_err(|e| Error::Validate { source: e })?;
    Ok(database_config)
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("sqlx error: {source}"))]
    Sqlx { source: sqlx::Error },
    #[snafu(display("validate error: {source}"))]
    Validate { source: validator::ValidationErrors },
    #[snafu(display("category: {category}, error: {message}"))]
    Common { category: String, message: String },
}

impl From<Error> for BaseError {
    fn from(source: Error) -> Self {
        let err = match source {
            Error::Sqlx { source } => BaseError::new(source)
                .with_sub_category("sqlx")
                .with_exception(true),
            Error::Validate { source } => BaseError::new(source)
                .with_sub_category("validate")
                .with_exception(true),
            Error::Common { message, .. } => BaseError::new(message).with_exception(true),
        };
        err.with_category("sql")
    }
}

#[derive(Debug, Default)]
pub struct PoolStat {
    connected: AtomicU32,
    executions: AtomicUsize,
    idle_for: AtomicU64,
}

impl PoolStat {
    pub fn stat(&self) -> (u32, usize, u64) {
        let connected = self.connected.swap(0, Ordering::Relaxed);
        let executions = self.executions.swap(0, Ordering::Relaxed);
        let idle_for = self.idle_for.swap(0, Ordering::Relaxed);
        (connected, executions, idle_for)
    }
}

type Result<T> = std::result::Result<T, Error>;

pub async fn new_mysql_pool(
    config: &Config,
    pool_stat: Option<Arc<PoolStat>>,
) -> Result<MySqlPool> {
    let database_config = new_database_config(config)?;
    let password = database_config.password.clone().unwrap_or_default();
    let url = database_config.url.replace(&password, "***");
    let category = "sqlx";
    info!(category, url, "connect to database");

    let mut options = PoolOptions::new()
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
                    stat.connected.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                })
            })
            .before_acquire(move |_conn, meta| {
                let stat = before_acquire_pool_stat.clone();
                Box::pin(async move {
                    let idle = meta.idle_for.as_secs();
                    info!(category, age = meta.age.as_secs(), idle, "before acquire");
                    stat.executions.fetch_add(1, Ordering::Relaxed);
                    stat.idle_for.fetch_add(idle, Ordering::Relaxed);
                    Ok(true)
                })
            });
    }

    let pool = options
        .connect(database_config.url.as_str())
        .await
        .map_err(|e| Error::Sqlx { source: e })?;
    Ok(pool)
}
