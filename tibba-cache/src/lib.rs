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
use std::time::Duration;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_util::parse_uri;
use validator::Validate;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("category: {category}, {message}"))]
    Common { category: String, message: String },
    #[snafu(display("{source}"))]
    SingleBuild { source: deadpool_redis::BuildError },
    #[snafu(display("{source}"))]
    ClusterBuild {
        source: deadpool_redis::cluster::CreatePoolError,
    },
    #[snafu(display("category: {category}, {source}"))]
    Redis {
        category: String,
        source: deadpool_redis::redis::RedisError,
    },
    #[snafu(display("{source}"))]
    Compression { source: tibba_util::Error },
    #[snafu(display("category: {category}, {source}"))]
    Url {
        category: String,
        source: url::ParseError,
    },
    #[snafu(display("category: {category}, {source}"))]
    Validate {
        category: String,
        source: validator::ValidationErrors,
    },
}

type Result<T> = std::result::Result<T, Error>;

// RedisConfig struct defines Redis-specific configuration
// with validation rules for connection parameters
#[derive(Debug, Clone, Default, Validate)]
pub struct RedisConfig {
    // redis nodes
    #[validate(length(min = 1))]
    pub nodes: Vec<String>,
    // pool size
    pub pool_size: u32,
    // connection timeout
    pub connection_timeout: Duration,
    // wait timeout
    pub wait_timeout: Duration,
    // recycle timeout
    pub recycle_timeout: Duration,
    // password
    pub password: Option<String>,
}

fn default_pool_size() -> u32 {
    10
}

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
    password: Option<String>,
}

// Creates a new RedisConfig instance from the configuration
// Parses Redis URI and extracts connection parameters
fn new_redis_config(config: &Config) -> Result<RedisConfig> {
    let uri = config.get_str("uri", "");
    let parsed = parse_uri::<RedisParams>(&uri).map_err(|e| Error::Common {
        category: "redis".to_string(),
        message: e.to_string(),
    })?;
    let nodes = parsed
        .host_strings()
        .iter()
        .map(|item| format!("redis://{item}"))
        .collect();
    let query = parsed.query;
    let redis_config = RedisConfig {
        nodes,
        pool_size: query.pool_size,
        connection_timeout: query.connection_timeout.unwrap_or(Duration::from_secs(3)),
        wait_timeout: query.wait_timeout.unwrap_or(Duration::from_secs(3)),
        recycle_timeout: query.recycle_timeout.unwrap_or(Duration::from_secs(60)),
        password: query.password,
    };
    redis_config.validate().map_err(|e| Error::Validate {
        category: "redis".to_string(),
        source: e,
    })?;
    Ok(redis_config)
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Common { category, message } => {
                BaseError::new(message).with_sub_category(&category)
            }
            Error::SingleBuild { source } => BaseError::new(source)
                .with_sub_category("single_build")
                .with_status(500)
                .with_exception(true),
            Error::ClusterBuild { source } => BaseError::new(source)
                .with_sub_category("cluster_build")
                .with_status(500)
                .with_exception(true),
            Error::Redis { category, source } => BaseError::new(source)
                .with_sub_category(&category)
                .with_status(500)
                .with_exception(true),
            Error::Compression { source } => BaseError::new(source)
                .with_sub_category("compression")
                .with_exception(true),
            Error::Url { category, source } => BaseError::new(source)
                .with_sub_category(&category)
                .with_status(500)
                .with_exception(true),
            Error::Validate { category, source } => {
                BaseError::new(source).with_sub_category(&category)
            }
        };
        err.with_category("cache")
    }
}

mod cache;
mod pool;
mod ttl_lru_store;
mod two_level_store;

pub use cache::*;
pub use pool::*;
pub use ttl_lru_store::*;
pub use two_level_store::*;
