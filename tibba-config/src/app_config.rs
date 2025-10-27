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
use config::{Config as RawConfig, ConfigError, Environment, File, FileFormat};
use parse_size::parse_size;
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

fn map_err(e: ConfigError) -> Error {
    Error::Config {
        category: "config".to_string(),
        source: e,
    }
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

    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        if key.is_empty() {
            return self.prefix.clone();
        }

        format!("{}.{}", self.prefix, key)
    }

    /// Try to deserialize the entire configuration into the requested type.
    pub fn try_deserialize<'de, T: Deserialize<'de>>(&self) -> Result<T> {
        let key = &self.get_key("");
        self.settings.get(key).map_err(map_err)
    }

    /// Retrieves a value, returning a default if not found or type mismatch.
    pub fn get<'de, T: Deserialize<'de>>(&self, key: &str) -> Result<T> {
        let key = &self.get_key(key);
        self.settings.get(key).map_err(map_err)
    }

    /// Retrieves a string value, returning a default if not found or type mismatch.
    pub fn get_string(&self, key: &str) -> Result<String> {
        let key = &self.get_key(key);
        self.settings.get_string(key).map_err(map_err)
    }

    /// Retrieves an integer value, returning a default if not found or type mismatch.
    pub fn get_int(&self, key: &str) -> Result<i64> {
        let key = &self.get_key(key);
        self.settings.get_int(key).map_err(map_err)
    }

    pub fn get_float(&self, key: &str) -> Result<f64> {
        let key = &self.get_key(key);
        self.settings.get_float(key).map_err(map_err)
    }

    /// Retrieves a boolean value, returning a default if not found or type mismatch.
    pub fn get_bool(&self, key: &str) -> Result<bool> {
        let key = &self.get_key(key);
        self.settings.get_bool(key).map_err(map_err)
    }

    /// Retrieves a Duration value, returning a default if not found or parsing fails.
    /// Note: `config` crate can deserialize human-readable strings ("10s", "1h") into `Duration`
    /// if the `humantime` feature is enabled on the `config` crate.
    pub fn get_duration(&self, key: &str) -> Result<Duration> {
        let key = &self.get_key(key);
        if let Ok(duration_str) = self.settings.get_string(key)
            && let Ok(duration) = humantime::parse_duration(&duration_str)
        {
            return Ok(duration);
        }

        // Fallback: try to parse as u64 seconds
        let seconds = self.settings.get_int(key).map_err(map_err)?;
        Ok(Duration::from_secs(seconds as u64))
    }

    /// Retrieves a byte size value, returning a default if not found or parsing fails.
    pub fn get_byte_size(&self, key: &str) -> Result<usize> {
        let key = &self.get_key(key);
        let value = self.settings.get_string(key).map_err(map_err)?;
        let size = parse_size(value).map_err(|e| Error::ParseSize {
            category: "config".to_string(),
            source: e,
        })?;
        Ok(size as usize)
    }

    /// Create a new sub-config with a given prefix.
    pub fn sub_config(&self, prefix: &str) -> Config {
        if prefix.is_empty() {
            return self.clone();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::time::Duration;

    fn create_test_config() -> Config {
        let toml_data = r#"
            # String values
            app_name = "test_app"
            empty_string = ""
            
            # Integer values
            port = 8080
            negative_number = -42
            
            # Boolean values
            debug = true
            production = false
            
            # Duration values - human readable
            timeout = "60s"
            cache_ttl = "5m"
            session_duration = "2h"
            cleanup_interval = "1d"
            
            # Duration values - numeric (seconds)
            numeric_timeout = 120
            
            # Byte size values
            max_file_size = "10MB"
            buffer_size = "1KB"
            
            # Nested configuration
            [database]
            host = "localhost"
            port = 5432
            timeout = "30s"
            
            [cache]
            enabled = true
            ttl = "10m"
            max_size = "100MB"
        "#;

        Config::new(vec![toml_data], Some("TEST")).unwrap()
    }

    #[test]
    fn test_config_creation() {
        let config = create_test_config();
        assert_eq!(config.env_prefix, "TEST");
        assert_eq!(config.prefix, "");
    }

    #[test]
    fn test_get_str() {
        let config = create_test_config();

        // Existing values
        assert_eq!(config.get_string("app_name").unwrap(), "test_app");
    }

    #[test]
    fn test_get_int() {
        let config = create_test_config();

        // Existing values
        assert_eq!(config.get_int("port").unwrap(), 8080);
        assert_eq!(config.get_int("negative_number").unwrap(), -42);
    }

    #[test]
    fn test_get_bool() {
        let config = create_test_config();

        // Existing values
        assert_eq!(config.get_bool("debug").unwrap(), true);
        assert_eq!(config.get_bool("production").unwrap(), false);
    }

    #[test]
    fn test_get_duration_human_readable() {
        let config = create_test_config();

        // Test various human-readable duration formats
        assert_eq!(
            config.get_duration("timeout").unwrap(),
            Duration::from_secs(60)
        );
        assert_eq!(
            config.get_duration("cache_ttl").unwrap(),
            Duration::from_secs(300)
        ); // 5 minutes
        assert_eq!(
            config.get_duration("session_duration").unwrap(),
            Duration::from_secs(7200)
        ); // 2 hours
        assert_eq!(
            config.get_duration("cleanup_interval").unwrap(),
            Duration::from_secs(86400)
        ); // 1 day
    }

    #[test]
    fn test_get_duration_numeric() {
        let config = create_test_config();

        // Test numeric duration (seconds)
        assert_eq!(
            config.get_duration("numeric_timeout").unwrap(),
            Duration::from_secs(120)
        );
    }
    #[test]
    fn test_get_byte_size() {
        let config = create_test_config();

        // Test byte size parsing
        assert_eq!(config.get_byte_size("max_file_size").unwrap(), 10_000_000); // 10MB
        assert_eq!(config.get_byte_size("buffer_size").unwrap(), 1_000); // 1KB
    }

    #[test]
    fn test_sub_config() {
        let config = create_test_config();

        #[derive(Deserialize)]
        struct DatabaseConfig {
            host: String,
            port: i64,
            #[serde(with = "humantime_serde")]
            timeout: Duration,
        }
        let database_config = config.get::<DatabaseConfig>("database").unwrap();
        assert_eq!(database_config.host, "localhost");
        assert_eq!(database_config.port, 5432);
        assert_eq!(database_config.timeout, Duration::from_secs(30));

        // Test database sub-config
        let db_config = config.sub_config("database");
        assert_eq!(db_config.prefix, "database");
        assert_eq!(db_config.get_string("host").unwrap(), "localhost");
        assert_eq!(db_config.get_int("port").unwrap(), 5432);
        assert_eq!(
            db_config.get_duration("timeout").unwrap(),
            Duration::from_secs(30)
        );

        // Test cache sub-config
        let cache_config = config.sub_config("cache");
        assert_eq!(cache_config.prefix, "cache");
        assert_eq!(cache_config.get_bool("enabled").unwrap(), true);
        assert_eq!(
            cache_config.get_duration("ttl").unwrap(),
            Duration::from_secs(600)
        ); // 10 minutes
        assert_eq!(cache_config.get_byte_size("max_size").unwrap(), 100_000_000); // 100MB
    }

    #[test]
    fn test_nested_sub_config() {
        let config = create_test_config();

        // Test nested sub-config
        let db_config = config.sub_config("database");
        let nested_config = db_config.sub_config("connection");
        assert_eq!(nested_config.prefix, "database.connection");
    }

    #[test]
    fn test_get_generic() {
        let config = create_test_config();

        // Test generic get method with different types
        assert_eq!(config.get::<String>("app_name").unwrap(), "test_app");
        assert_eq!(config.get::<i64>("port").unwrap(), 8080);
        assert_eq!(config.get::<bool>("debug").unwrap(), true);

        // Test error case
        assert!(config.get::<String>("non_existent").is_err());
    }

    #[test]
    fn test_empty_config() {
        let config = Config::new(vec![""], None).unwrap();

        // All methods should return defaults for empty config
        assert_eq!(config.get_int("any_key").unwrap(), 42);
        assert_eq!(config.get_bool("any_key").unwrap(), true);
        assert_eq!(
            config.get_duration("any_key").unwrap(),
            Duration::from_secs(60)
        );
        assert_eq!(config.get_byte_size("any_key").unwrap(), 1024);
    }

    #[test]
    fn test_environment_variable_override() {
        // This test would require setting environment variables
        // For now, we'll just test that the config can be created with env prefix
        let config = Config::new(vec![""], Some("MYAPP")).unwrap();
        assert_eq!(config.env_prefix, "MYAPP");
    }

    #[test]
    fn test_multiple_config_sources() {
        let config1 = r#"
            app_name = "config1"
            port = 8080
        "#;

        let config2 = r#"
            app_name = "config2"
            debug = true
        "#;

        // Later configs should override earlier ones
        let config = Config::new(vec![config1, config2], None).unwrap();
        assert_eq!(config.get_string("app_name").unwrap(), "config2"); // Overridden
        assert_eq!(config.get_int("port").unwrap(), 8080); // From config1
        assert_eq!(config.get_bool("debug").unwrap(), true); // From config2
    }
}
