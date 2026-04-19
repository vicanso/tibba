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
    #[snafu(display("single connect error: {source}"))]
    SingleConnect { source: deadpool_redis::PoolError },
    #[snafu(display("cluster connect error: {source}"))]
    ClusterConnect {
        source: deadpool_redis::cluster::PoolError,
    },
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
    // idle timeout
    pub idle_timeout: Duration,
    // password
    pub password: Option<String>,
    // max connection age
    pub max_conn_age: Duration,
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
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    max_conn_age: Option<Duration>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    idle_timeout: Option<Duration>,
    password: Option<String>,
}

// Creates a new RedisConfig instance from the configuration
// Parses Redis URI and extracts connection parameters
fn new_redis_config(config: &Config) -> Result<RedisConfig> {
    let uri = config.get_string("uri").context(ConfigSnafu)?;
    let parsed = parse_uri::<RedisParams>(&uri).context(ParseUriSnafu)?;
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
        // 检测请求是否可用的超时时间，默认300ms
        recycle_timeout: query.recycle_timeout.unwrap_or(Duration::from_millis(300)),
        max_conn_age: query.max_conn_age.unwrap_or(Duration::from_secs(24 * 3600)),
        // 由于pool本身没有idle timeout处理，因此现在的模块在复用前判断，需要根据redis server设置调整，默认10分钟
        idle_timeout: query.idle_timeout.unwrap_or(Duration::from_secs(10 * 60)),
        password: query.password,
    };
    redis_config
        .validate()
        .context(ValidateSnafu { category: "redis" })?;
    Ok(redis_config)
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        // Infrastructure errors: unreachable Redis → 500 + exception
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

/// Tracing target for all log events in this crate.
/// Use `RUST_LOG=tibba_cache=info` (or `debug`) to filter these logs.
pub(crate) const LOG_TARGET: &str = "tibba_cache";

mod cache;
mod pool;
mod ttl_lru_store;
mod two_level_store;

pub use cache::*;
pub use pool::*;
pub use ttl_lru_store::*;
pub use two_level_store::*;
