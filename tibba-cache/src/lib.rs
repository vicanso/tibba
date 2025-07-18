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
use std::time::Duration;
use substring::Substring;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_error::new_error;
use url::Url;
use validator::Validate;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("category: {category}, error: {message}"))]
    Common { category: String, message: String },
    #[snafu(display("{source}"))]
    SingleBuild { source: deadpool_redis::BuildError },
    #[snafu(display("{source}"))]
    ClusterBuild {
        source: deadpool_redis::cluster::CreatePoolError,
    },
    #[snafu(display("category: {category}, error: {source}"))]
    Redis {
        category: String,
        source: deadpool_redis::redis::RedisError,
    },
    #[snafu(display("{source}"))]
    Compression { source: tibba_util::Error },
    #[snafu(display("category: {category}, error: {source}"))]
    Url {
        category: String,
        source: url::ParseError,
    },
    #[snafu(display("category: {category}, error: {source}"))]
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

// Creates a new RedisConfig instance from the configuration
// Parses Redis URI and extracts connection parameters
fn new_redis_config(config: &Config) -> Result<RedisConfig> {
    let uri = config.get_from_env_first("uri", None);
    if uri.is_empty() {
        return Err(Error::Common {
            category: "redis".to_string(),
            message: "uri is empty".to_string(),
        });
    }
    let start = if let Some(index) = uri.find('@') {
        index + 1
    } else {
        uri.find("//").unwrap_or_default() + 2
    };

    let mut host = uri.substring(start, uri.len());
    if let Some(end) = host.find('/') {
        host = host.substring(0, end);
    }
    let mut nodes = vec![];
    for item in host.split(',') {
        nodes.push(uri.replace(host, item));
    }
    let info = Url::parse(&nodes[0]).map_err(|e| Error::Url {
        category: "redis".to_string(),
        source: e,
    })?;
    let mut redis_config = RedisConfig {
        nodes,
        pool_size: 10,
        connection_timeout: Duration::from_secs(3),
        wait_timeout: Duration::from_secs(3),
        recycle_timeout: Duration::from_secs(60),
        password: info.password().map(|v| v.to_string()),
    };
    for (key, value) in info.query_pairs() {
        match key.to_string().as_str() {
            "pool_size" => {
                if let Ok(num) = value.parse::<u32>() {
                    redis_config.pool_size = num;
                }
            }
            "connection_timeout" => {
                if let Ok(value) = Config::parse_duration(&value) {
                    redis_config.connection_timeout = value;
                }
            }
            "wait_timeout" => {
                if let Ok(value) = Config::parse_duration(&value) {
                    redis_config.wait_timeout = value;
                }
            }
            "recycle_timeout" => {
                if let Ok(value) = Config::parse_duration(&value) {
                    redis_config.recycle_timeout = value;
                }
            }
            _ => (),
        }
    }
    redis_config.validate().map_err(|e| Error::Validate {
        category: "redis".to_string(),
        source: e,
    })?;
    Ok(redis_config)
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let error_category = "cache";
        match val {
            Error::Common { category, message } => new_error(message)
                .with_category(error_category)
                .with_sub_category(&category),
            Error::SingleBuild { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("single_build")
                .with_status(500)
                .with_exception(true),
            Error::ClusterBuild { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("cluster_build")
                .with_status(500)
                .with_exception(true),
            Error::Redis { category, source } => new_error(source)
                .with_category(error_category)
                .with_sub_category(&category)
                .with_status(500)
                .with_exception(true),
            Error::Compression { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("compression")
                .with_exception(true),
            Error::Url { category, source } => new_error(source)
                .with_category(error_category)
                .with_sub_category(&category)
                .with_status(500)
                .with_exception(true),
            Error::Validate { category, source } => new_error(source)
                .with_category(error_category)
                .with_sub_category(&category),
        }
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
