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
use std::time::Duration;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_error::new_error;
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
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
}

// Creates a new DatabaseConfig instance from the configuration
fn new_database_config(config: &Config) -> Result<DatabaseConfig> {
    let origin_url = config.get_from_env_first("url", None);
    let mut url = origin_url.clone();
    let info = Url::parse(&url).unwrap();
    let mut max_connections = 10;
    let mut min_connections = 2;
    let mut connect_timeout = Duration::from_secs(3);
    let mut acquire_timeout = Duration::from_secs(5);
    let mut idle_timeout = Duration::from_secs(60);

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
                "acquire_timeout" => {
                    if let Ok(value) = Config::parse_duration(&value) {
                        acquire_timeout = value;
                    }
                }
                "idle_timeout" => {
                    if let Ok(value) = Config::parse_duration(&value) {
                        idle_timeout = value;
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
        acquire_timeout,
        idle_timeout,
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
}

impl From<Error> for BaseError {
    fn from(source: Error) -> Self {
        let error_category = "sql";
        match source {
            Error::Sqlx { source } => {
                let he = new_error(&source.to_string())
                    .with_category(error_category)
                    .with_sub_category("sqlx")
                    .with_exception(true);
                he.into()
            }
            Error::Validate { source } => {
                let he = new_error(&source.to_string())
                    .with_category(error_category)
                    .with_sub_category("validate")
                    .with_exception(true);
                he.into()
            }
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

pub async fn new_mysql_pool(config: &Config) -> Result<MySqlPool> {
    let database_config = new_database_config(config)?;
    let pool = PoolOptions::new()
        .connect(database_config.url.as_str())
        .await
        .map_err(|e| Error::Sqlx { source: e })?;
    Ok(pool)
}
