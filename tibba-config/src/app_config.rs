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
use config::{File, FileFormat, FileSourceString};
use std::collections::HashMap;
use std::env;
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;
// Config struct represents the application configuration
// It manages configuration settings with environment variable support
#[derive(Debug, Clone, Default)]
pub struct Config {
    // Prefix for environment variables
    env_prefix: String,
    // Prefix for configuration keys
    prefix: String,
    // Nested HashMap storing configuration values
    settings: HashMap<String, HashMap<String, String>>,
}

impl Config {
    // Sets a new prefix and returns a new Config instance
    fn set_prefix(&self, prefix: &str) -> Config {
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
    pub fn convert_string_to_i32(value: &str) -> i32 {
        if let Ok(result) = value.parse::<i32>() {
            return result;
        }
        0
    }
    pub fn convert_string_to_bool(value: &str) -> bool {
        if let Ok(result) = value.parse::<bool>() {
            return result;
        }
        false
    }
    pub fn parse_duration(value: &str) -> Result<Duration> {
        humantime::parse_duration(value).map_err(|e| Error::ParseDuration {
            category: "config".to_string(),
            source: e,
        })
    }
    // Retrieves a configuration value by key with optional default value
    pub fn get(&self, key: &str, default_value: Option<String>) -> String {
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
    pub fn get_int(&self, key: &str, default_value: Option<i32>) -> i32 {
        let value = self.get(key, None);
        if !value.is_empty() {
            return Self::convert_string_to_i32(&value);
        }
        default_value.unwrap_or_default()
    }
    pub fn get_duration(&self, key: &str, default_value: Option<Duration>) -> Result<Duration> {
        let value = self.get(key, None);
        if !value.is_empty() {
            return Self::parse_duration(&value);
        }
        Ok(default_value.unwrap_or_default())
    }
    pub fn get_bool(&self, key: &str, default_value: Option<bool>) -> bool {
        let value = self.get(key, None);
        if !value.is_empty() {
            return Self::convert_string_to_bool(&value);
        }
        default_value.unwrap_or_default()
    }
    // Retrieves value from environment variable first, falls back to config file
    pub fn get_from_env_first(&self, key: &str, default_value: Option<String>) -> String {
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
    pub fn get_int_from_env_first(&self, key: &str, default_value: Option<i32>) -> i32 {
        let value = self.get_from_env_first(key, None);
        if !value.is_empty() {
            return Self::convert_string_to_i32(&value);
        }
        default_value.unwrap_or_default()
    }
    pub fn get_bool_from_env_first(&self, key: &str, default_value: Option<bool>) -> bool {
        let value = self.get_from_env_first(key, None);
        if !value.is_empty() {
            return Self::convert_string_to_bool(&value);
        }
        default_value.unwrap_or_default()
    }
    // Similar to get_from_env_first but converts the value to Duration
    pub fn get_duration_from_env_first(
        &self,
        key: &str,
        default_value: Option<Duration>,
    ) -> Duration {
        let value = self.get_from_env_first(key, None);
        let v = default_value.unwrap_or_default();
        if !value.is_empty() {
            return Self::parse_duration(&value).unwrap_or(v);
        }
        v
    }
    /// Create a new sub config
    pub fn sub_config(&self, prefix: &str) -> Config {
        self.clone().set_prefix(prefix)
    }
}

// Creates a new File source from TOML string data
fn new_source(data: &str) -> File<FileSourceString, FileFormat> {
    File::from_str(data, FileFormat::Toml)
}

// Creates a new AppConfig instance from multiple TOML configuration strings
pub fn new_config(data: Vec<&str>, env_prefix: Option<&str>) -> Result<Config> {
    let mut builder = config::Config::builder();
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
    Ok(Config {
        env_prefix: env_prefix.unwrap_or_default().to_string(),
        settings,
        ..Default::default()
    })
}
