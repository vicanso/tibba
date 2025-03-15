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

use super::Error;
use config::{Config, File, FileFormat, FileSourceString};
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use substring::Substring;
use tibba_validator::x_listen_addr;
use url::Url;
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

// Helper function to convert string to i32, returns 0 if parsing fails
fn convert_string_to_i32(value: String) -> i32 {
    if let Ok(result) = value.parse::<i32>() {
        return result;
    }
    0
}

// fn convert_string_to_bool(value: String) -> bool {
//     if let Ok(result) = value.parse::<bool>() {
//         return result;
//     }
//     false
// }

// AppConfig struct represents the application configuration
// It manages configuration settings with environment variable support
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    // Prefix for environment variables
    env_prefix: String,
    // Prefix for configuration keys
    prefix: String,
    // Nested HashMap storing configuration values
    settings: HashMap<String, HashMap<String, String>>,
}

impl AppConfig {
    // Sets a new prefix and returns a new AppConfig instance
    fn set_prefix(&self, prefix: &str) -> AppConfig {
        let mut config = self.clone();
        config.prefix = prefix.to_string();
        config
    }
    // Constructs the full configuration key using the prefix
    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        format!("{}.{key}", self.prefix)
    }
    // Retrieves a configuration value by key with optional default value
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
    // fn get_int(&self, key: &str, default_value: Option<i32>) -> i32 {
    //     let value = self.get(key, None);
    //     if !value.is_empty() {
    //         return convert_string_to_i32(value);
    //     }
    //     default_value.unwrap_or_default()
    // }
    // fn get_duration(&self, key: &str, default_value: Option<Duration>) -> Result<Duration> {
    //     let value = self.get(key, None);
    //     if !value.is_empty() {
    //         return humantime::parse_duration(&value).context(ParseDurationSnafu {
    //             category: key.to_string(),
    //         });
    //     }
    //     Ok(default_value.unwrap_or_default())
    // }
    // fn get_bool(&self, key: &str, default_value: Option<bool>) -> bool {
    //     let value = self.get(key, None);
    //     if !value.is_empty() {
    //         return convert_string_to_bool(value);
    //     }
    //     default_value.unwrap_or_default()
    // }
    // Retrieves value from environment variable first, falls back to config file
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
    // Similar to get_from_env_first but converts the value to integer
    fn get_int_from_env_first(&self, key: &str, default_value: Option<i32>) -> i32 {
        let value = self.get_from_env_first(key, None);
        if !value.is_empty() {
            return convert_string_to_i32(value);
        }
        default_value.unwrap_or_default()
    }
    // fn get_bool_from_env_first(&self, key: &str, default_value: Option<bool>) -> bool {
    //     let value = self.get_from_env_first(key, None);
    //     if !value.is_empty() {
    //         return convert_string_to_bool(value);
    //     }
    //     default_value.unwrap_or_default()
    // }
    // Similar to get_from_env_first but converts the value to Duration
    fn get_duration_from_env_first(&self, key: &str, default_value: Option<Duration>) -> Duration {
        let value = self.get_from_env_first(key, None);
        let v = default_value.unwrap_or_default();
        if !value.is_empty() {
            return humantime::parse_duration(&value).unwrap_or(v);
        }
        v
    }
}

// Creates a new File source from TOML string data
fn new_source(data: &str) -> File<FileSourceString, FileFormat> {
    File::from_str(data, FileFormat::Toml)
}

// Creates a new AppConfig instance from multiple TOML configuration strings
pub fn new_app_config(data: Vec<&str>, env_prefix: Option<&str>) -> Result<AppConfig> {
    let mut builder = Config::builder();
    for d in data {
        if !d.is_empty() {
            builder = builder.add_source(new_source(d));
        }
    }
    let settings = builder
        .build()
        .map_err(|e| Error::Config {
            category: "config_builder".to_string(),
            source: e,
        })?
        .try_deserialize::<HashMap<String, HashMap<String, String>>>()
        .map_err(|e| Error::Config {
            category: "config_deserialize".to_string(),
            source: e,
        })?;
    Ok(AppConfig {
        env_prefix: env_prefix.unwrap_or_default().to_string(),
        settings,
        ..Default::default()
    })
}

// BasicConfig struct defines the basic application settings
// with validation rules for each field
#[derive(Debug, Clone, Default, Validate)]
pub struct BasicConfig {
    // listen address
    #[validate(custom(function = "x_listen_addr"))]
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
        basic_config.validate().map_err(|e| Error::Validate {
            category: "basic".to_string(),
            source: e,
        })?;
        Ok(basic_config)
    }
}

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
}

impl AppConfig {
    // Creates a new RedisConfig instance from the configuration
    // Parses Redis URI and extracts connection parameters
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
        redis_config.validate().map_err(|e| Error::Validate {
            category: "redis".to_string(),
            source: e,
        })?;
        Ok(redis_config)
    }
}
