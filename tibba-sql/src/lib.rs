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

use snafu::Snafu;
use sqlx::MySqlPool;
use sqlx::pool::PoolOptions;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_error::new_error;
use tracing::info;
use url::Url;
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

// Creates a new DatabaseConfig instance from the configuration
fn new_database_config(config: &Config) -> Result<DatabaseConfig> {
    let origin_url = config.get_from_env_first("uri", None);
    if origin_url.is_empty() {
        return Err(Error::Common {
            category: "config".to_string(),
            message: "uri is empty".to_string(),
        });
    }
    let mut url = origin_url.clone();
    let info = Url::parse(&url).unwrap();

    let mut max_connections = 10;
    let mut min_connections = 2;
    let mut connect_timeout = Duration::from_secs(3);
    let mut idle_timeout = Duration::from_secs(60);
    let mut max_lifetime = Duration::from_secs(6 * 60 * 60);
    let mut test_before_acquire = true;

    if let Some(query) = info.query() {
        url = url.replace(&format!("?{query}"), "");
        for (key, value) in info.query_pairs() {
            match key.to_string().as_str() {
                "max_connections" => {
                    let value = Config::convert_string_to_i32(&value);
                    if value > 0 {
                        max_connections = value as u32;
                    }
                }
                "min_connections" => {
                    let value = Config::convert_string_to_i32(&value);
                    if value > 0 {
                        min_connections = value as u32;
                    }
                }
                "connect_timeout" => {
                    if let Ok(value) = Config::parse_duration(&value) {
                        connect_timeout = value;
                    }
                }
                "idle_timeout" => {
                    if let Ok(value) = Config::parse_duration(&value) {
                        idle_timeout = value;
                    }
                }
                "max_lifetime" => {
                    if let Ok(value) = Config::parse_duration(&value) {
                        max_lifetime = value;
                    }
                }
                "test_before_acquire" => {
                    if let Ok(value) = value.parse::<bool>() {
                        test_before_acquire = value;
                    }
                }
                _ => {}
            }
        }
    }
    let database_config = DatabaseConfig {
        origin_url,
        url,
        max_connections,
        min_connections,
        connect_timeout,
        idle_timeout,
        max_lifetime,
        test_before_acquire,
        password: info.password().map(|v| v.to_string()),
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
            Error::Sqlx { source } => new_error(source)
                .with_sub_category("sqlx")
                .with_exception(true),
            Error::Validate { source } => new_error(source)
                .with_sub_category("validate")
                .with_exception(true),
            Error::Common { message, .. } => new_error(message).with_exception(true),
        };
        err.with_category("sql")
    }
}

#[derive(Debug, Default)]
pub struct PoolStat {
    connected: AtomicU32,
    executions: AtomicUsize,
    idle: AtomicU64,
}

impl PoolStat {
    pub fn stat(&self) -> (u32, usize, u64) {
        let connected = self.connected.load(Ordering::Relaxed);
        let executions = self.executions.swap(0, Ordering::Relaxed);
        let idle = self.idle.swap(0, Ordering::Relaxed);
        (connected, executions, idle)
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
    let after_connect_pool_stat = pool_stat.clone();
    let before_acquire_pool_stat = pool_stat.clone();
    let after_release_pool_stat = pool_stat.clone();
    let pool = PoolOptions::new()
        .after_connect(move |_conn, _meta| {
            Box::pin({
                let pool_stat = after_connect_pool_stat.clone();
                async move {
                    if let Some(pool_stat) = &pool_stat {
                        pool_stat.connected.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(())
                }
            })
        })
        .before_acquire(move |_conn, meta| {
            Box::pin({
                let pool_stat = before_acquire_pool_stat.clone();
                async move {
                    let idle = meta.idle_for.as_secs();
                    info!(category, age = meta.age.as_secs(), idle, "before acquire");
                    if let Some(pool_stat) = &pool_stat {
                        pool_stat.executions.fetch_add(1, Ordering::Relaxed);
                        pool_stat.idle.fetch_add(idle, Ordering::Relaxed);
                    }
                    Ok(true)
                }
            })
        })
        .after_release(move |_conn, meta| {
            let pool_stat = after_release_pool_stat.clone();
            Box::pin(async move {
                // Only check connections older than 6 hours.
                if meta.age.as_secs() < 6 * 60 * 60 {
                    return Ok(true);
                }
                let idle = meta.idle_for.as_secs();
                info!(
                    category,
                    age = meta.age.as_secs(),
                    idle,
                    "release connection"
                );
                if let Some(pool_stat) = &pool_stat {
                    pool_stat.connected.fetch_sub(1, Ordering::Relaxed);
                }
                Ok(false)
            })
        })
        .max_connections(database_config.max_connections)
        .min_connections(database_config.min_connections)
        .idle_timeout(database_config.idle_timeout)
        .max_lifetime(database_config.max_lifetime)
        .test_before_acquire(database_config.test_before_acquire)
        .connect(database_config.url.as_str())
        .await
        .map_err(|e| Error::Sqlx { source: e })?;
    Ok(pool)
}
