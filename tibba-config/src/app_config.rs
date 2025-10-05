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
use bytesize::ByteSize;
use config::{Config as RawConfig, Environment, File, FileFormat};
use serde::Deserialize;
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

/// Config struct represents the application configuration.
/// It wraps the `config::Config` instance to provide namespacing and convenience methods.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Prefix for environment variables (e.g., "APP").
    env_prefix: String,
    /// Prefix for configuration keys, used for sub-configs.
    prefix: String,
    settings: RawConfig,
}

impl Config {
    pub fn new(data: Vec<&str>, env_prefix: Option<&str>) -> Result<Self> {
        let env_prefix_str = env_prefix.unwrap_or_default();

        let mut builder = RawConfig::builder();

        // Add file sources
        for d in data {
            if !d.is_empty() {
                builder = builder.add_source(File::from_str(d, FileFormat::Toml));
            }
        }

        // Environment variables are used to override the configuration values.
        // For example, `APP_DATABASE_PORT=5433` will automatically override the `database.port` configuration.
        builder = builder.add_source(
            Environment::with_prefix(env_prefix_str)
                .prefix_separator("_")
                .separator("_"), // use `_` as the separator, for example `APP_DATABASE_HOST`
        );

        let settings = builder.build().map_err(|e| Error::Config {
            category: "builder".to_string(),
            source: e,
        })?;

        Ok(Self {
            env_prefix: env_prefix_str.to_string(),
            settings,
            prefix: "".to_string(),
        })
    }

    /// Retrieves a value, returning a default if not found or type mismatch.
    pub fn get<'de, T: Deserialize<'de>>(&self, key: &str) -> Result<T> {
        let full_key = if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}.{}", self.prefix, key)
        };
        self.settings.get(&full_key).map_err(|e| Error::Config {
            category: "config".to_string(),
            source: e,
        })
    }

    /// Retrieves a string value, returning a default if not found or type mismatch.
    pub fn get_str(&self, key: &str, default_value: &str) -> String {
        self.get(key).unwrap_or_else(|_| default_value.to_string())
    }

    /// Retrieves an integer value, returning a default if not found or type mismatch.
    pub fn get_int(&self, key: &str, default_value: i64) -> i64 {
        self.get(key).unwrap_or(default_value)
    }

    /// Retrieves a boolean value, returning a default if not found or type mismatch.
    pub fn get_bool(&self, key: &str, default_value: bool) -> bool {
        self.get(key).unwrap_or(default_value)
    }

    /// Retrieves a Duration value, returning a default if not found or parsing fails.
    /// Note: `config` crate can deserialize human-readable strings ("10s", "1h") into `Duration`
    /// if the `humantime` feature is enabled on the `config` crate.
    pub fn get_duration(&self, key: &str, default_value: Duration) -> Duration {
        self.get(key).unwrap_or(default_value)
    }

    /// Retrieves a byte size value, returning a default if not found or parsing fails.
    pub fn get_byte_size(&self, key: &str, default_value: usize) -> usize {
        self.get::<String>(key)
            .ok()
            .and_then(|s| s.parse::<ByteSize>().ok())
            .map(|bs| bs.as_u64() as usize)
            .unwrap_or(default_value)
    }

    /// Create a new sub-config with a given prefix.
    pub fn sub_config(&self, prefix: &str) -> Config {
        let new_prefix = if self.prefix.is_empty() {
            prefix.to_string()
        } else {
            format!("{}.{}", self.prefix, prefix)
        };

        Config {
            env_prefix: self.env_prefix.clone(),
            prefix: new_prefix,
            settings: self.settings.clone(),
        }
    }
}
