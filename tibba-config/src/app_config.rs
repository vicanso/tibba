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

use config::{Config, File, FileFormat, FileSourceString};
use snafu::{ResultExt, Snafu};
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use substring::Substring;
use url::Url;
use validator::Validate;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{category}, url parse error {source}"))]
    Url {
        category: String,
        source: url::ParseError,
    },
    #[snafu(display("{category}, config error {source}"))]
    Config {
        category: String,
        source: config::ConfigError,
    },
    #[snafu(display("{category}, validate error {source}"))]
    Validate {
        category: String,
        source: validator::ValidationErrors,
    },
    #[snafu(display("{category}, parse duration error {source}"))]
    ParseDuration {
        category: String,
        source: humantime::DurationError,
    },
}

type Result<T> = std::result::Result<T, Error>;

fn convert_string_to_i32(value: String) -> i32 {
    if let Ok(result) = value.parse::<i32>() {
        return result;
    }
    0
}

fn convert_string_to_bool(value: String) -> bool {
    if let Ok(result) = value.parse::<bool>() {
        return result;
    }
    false
}

#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    env_prefix: String,
    prefix: String,
    settings: HashMap<String, HashMap<String, String>>,
}

impl AppConfig {
    fn set_prefix(&self, prefix: &str) -> AppConfig {
        let mut config = self.clone();
        config.prefix = prefix.to_string();
        config
    }
    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        format!("{}.{key}", self.prefix)
    }
    fn get(&self, key: &str, default_value: Option<String>) -> String {
        let mut s = "".to_string();
        let k = self.get_key(key);
        let arr: Vec<&str> = k.split('.').collect();
        if arr.len() == 2 {
            if let Some(value) = self.settings.get(arr[0]) {
                if let Some(v) = value.get(arr[1]) {
                    s = v.clone();
                }
            }
        }
        if !s.is_empty() {
            return s;
        }
        default_value.unwrap_or(s)
    }
    fn get_int(&self, key: &str, default_value: Option<i32>) -> i32 {
        let value = self.get(key, None);
        if !value.is_empty() {
            return convert_string_to_i32(value);
        }
        default_value.unwrap_or_default()
    }
    fn get_duration(&self, key: &str, default_value: Option<Duration>) -> Result<Duration> {
        let value = self.get(key, None);
        if !value.is_empty() {
            return humantime::parse_duration(&value).context(ParseDurationSnafu {
                category: key.to_string(),
            });
        }
        Ok(default_value.unwrap_or_default())
    }
    fn get_bool(&self, key: &str, default_value: Option<bool>) -> bool {
        let value = self.get(key, None);
        if !value.is_empty() {
            return convert_string_to_bool(value);
        }
        default_value.unwrap_or_default()
    }
    fn get_from_env_first(&self, key: &str, default_value: Option<String>) -> String {
        let k = self.get_key(key);
        let mut env_key = k.replace('.', "_").to_uppercase();
        if !self.env_prefix.is_empty() {
            env_key = format!("{}_{env_key}", self.env_prefix);
        }
        if let Ok(value) = env::var(env_key) {
            return value;
        }
        self.get(key, default_value)
    }
    fn get_int_from_env_first(&self, key: &str, default_value: Option<i32>) -> i32 {
        let value = self.get_from_env_first(key, None);
        if !value.is_empty() {
            return convert_string_to_i32(value);
        }
        default_value.unwrap_or_default()
    }
    fn get_bool_from_env_first(&self, key: &str, default_value: Option<bool>) -> bool {
        let value = self.get_from_env_first(key, None);
        if !value.is_empty() {
            return convert_string_to_bool(value);
        }
        default_value.unwrap_or_default()
    }
    fn get_duration_from_env_first(&self, key: &str, default_value: Option<Duration>) -> Duration {
        let value = self.get_from_env_first(key, None);
        let v = default_value.unwrap_or_default();
        if !value.is_empty() {
            return humantime::parse_duration(&value).unwrap_or(v);
        }
        v
    }
}

fn new_source(data: &str) -> File<FileSourceString, FileFormat> {
    File::from_str(data, FileFormat::Toml)
}

pub fn new_app_config(data: Vec<&str>, env_prefix: Option<&str>) -> Result<AppConfig> {
    let mut builder = Config::builder();
    for d in data {
        if !d.is_empty() {
            builder = builder.add_source(new_source(d));
        }
    }
    let settings = builder
        .build()
        .context(ConfigSnafu {
            category: "config_builder".to_string(),
        })?
        .try_deserialize::<HashMap<String, HashMap<String, String>>>()
        .context(ConfigSnafu {
            category: "config_deserialize".to_string(),
        })?;
    Ok(AppConfig {
        env_prefix: env_prefix.unwrap_or_default().to_string(),
        settings,
        ..Default::default()
    })
}

#[derive(Debug, Clone, Default, Validate)]
pub struct BasicConfig {
    // listen address
    #[validate(length(min = 1))]
    pub listen: String,
    // processing limit
    #[validate(range(min = 0, max = 100000))]
    pub processing_limit: i32,
    // timeout
    pub timeout: Duration,
    // secret
    pub secret: String,
}

impl AppConfig {
    /// Create a new basic config, if the config is invalid, it will panic
    pub fn new_basic_config(&self) -> Result<BasicConfig> {
        let config = self.clone().set_prefix("basic");
        let timeout = config.get_duration_from_env_first("timeout", Some(Duration::from_secs(60)));
        let basic_config = BasicConfig {
            listen: config.get_from_env_first("listen", None),
            processing_limit: config.get_int_from_env_first("processing_limit", Some(5000)),
            timeout,
            secret: config.get_from_env_first("secret", None),
        };
        basic_config.validate().context(ValidateSnafu {
            category: "basic".to_string(),
        })?;
        Ok(basic_config)
    }
}

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
}

impl AppConfig {
    pub fn new_redis_config(&self) -> Result<RedisConfig> {
        let config = self.clone().set_prefix("redis");
        let uri = config.get_from_env_first("uri", None);
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
        let info = Url::parse(&nodes[0]).context(UrlSnafu {
            category: "redis".to_string(),
        })?;
        let mut redis_config = RedisConfig {
            nodes,
            pool_size: 10,
            connection_timeout: Duration::from_secs(3),
            wait_timeout: Duration::from_secs(3),
            recycle_timeout: Duration::from_secs(60),
        };
        for (key, value) in info.query_pairs() {
            match key.to_string().as_str() {
                "pool_size" => {
                    if let Ok(num) = value.parse::<u32>() {
                        redis_config.pool_size = num;
                    }
                }
                "connection_timeout" => {
                    if let Ok(value) = humantime::parse_duration(&value) {
                        redis_config.connection_timeout = value;
                    }
                }
                "wait_timeout" => {
                    if let Ok(value) = humantime::parse_duration(&value) {
                        redis_config.wait_timeout = value;
                    }
                }
                "recycle_timeout" => {
                    if let Ok(value) = humantime::parse_duration(&value) {
                        redis_config.recycle_timeout = value;
                    }
                }
                _ => (),
            }
        }
        redis_config.validate().context(ValidateSnafu {
            category: "redis".to_string(),
        })?;
        Ok(redis_config)
    }
}
