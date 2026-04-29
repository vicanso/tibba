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

use super::{ConfigSnafu, Error, ParseSizeSnafu};
use config::{Config as RawConfig, Environment, File, FileFormat};
use parse_size::parse_size;
use serde::Deserialize;
use snafu::ResultExt;
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

/// 应用配置，封装底层 `config::Config`，提供命名空间与便捷读取方法。
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// 环境变量前缀，例如 "APP"，用于从环境变量中覆盖配置项。
    env_prefix: String,
    /// 子配置前缀，用于隔离不同模块的配置命名空间。
    prefix: String,
    settings: RawConfig,
}

impl Config {
    /// 从多个 TOML 字符串和可选的环境变量前缀构建配置。
    /// 后面的配置源会覆盖前面的同名配置项，环境变量优先级最高。
    /// 例如：`APP_DATABASE_PORT=5433` 会覆盖 TOML 中的 `database.port`。
    pub fn new(data: &[&str], env_prefix: Option<&str>) -> Result<Self> {
        let env_prefix_str = env_prefix.unwrap_or_default();

        let mut builder = RawConfig::builder();

        // 逐个加载 TOML 配置源，空字符串跳过
        for d in data {
            if !d.is_empty() {
                builder = builder.add_source(File::from_str(d, FileFormat::Toml));
            }
        }

        // 环境变量覆盖同名配置，使用 `_` 作为层级分隔符
        // 例如 `APP_DATABASE_HOST` 对应 `database.host`
        builder = builder.add_source(
            Environment::with_prefix(env_prefix_str)
                .prefix_separator("_")
                .separator("_"),
        );

        let settings = builder.build().context(ConfigSnafu {
            category: "builder",
        })?;

        Ok(Self {
            env_prefix: env_prefix_str.to_string(),
            settings,
            prefix: String::new(),
        })
    }

    /// 将前缀与键名拼接为完整的配置键路径。
    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        if key.is_empty() {
            return self.prefix.clone();
        }

        format!("{}.{}", self.prefix, key)
    }

    /// 将当前命名空间下的配置整体反序列化为指定类型。
    pub fn try_deserialize<'de, T: Deserialize<'de>>(&self) -> Result<T> {
        self.settings
            .get(&self.get_key(""))
            .context(ConfigSnafu { category: "config" })
    }

    /// 读取任意可反序列化类型的配置值。
    pub fn get<'de, T: Deserialize<'de>>(&self, key: &str) -> Result<T> {
        self.settings
            .get(&self.get_key(key))
            .context(ConfigSnafu { category: "config" })
    }

    /// 读取字符串类型的配置值。
    pub fn get_string(&self, key: &str) -> Result<String> {
        self.settings
            .get_string(&self.get_key(key))
            .context(ConfigSnafu { category: "config" })
    }

    /// 读取 i64 类型的整数配置值。
    pub fn get_int(&self, key: &str) -> Result<i64> {
        self.settings
            .get_int(&self.get_key(key))
            .context(ConfigSnafu { category: "config" })
    }

    /// 读取 f64 类型的浮点数配置值。
    pub fn get_float(&self, key: &str) -> Result<f64> {
        self.settings
            .get_float(&self.get_key(key))
            .context(ConfigSnafu { category: "config" })
    }

    /// 读取布尔类型的配置值。
    pub fn get_bool(&self, key: &str) -> Result<bool> {
        self.settings
            .get_bool(&self.get_key(key))
            .context(ConfigSnafu { category: "config" })
    }

    /// 读取时间长度配置值。
    /// 优先解析人类可读格式（如 "10s"、"1h"），失败则回退为纯数字（秒）。
    pub fn get_duration(&self, key: &str) -> Result<Duration> {
        let key = &self.get_key(key);
        if let Ok(duration_str) = self.settings.get_string(key)
            && let Ok(duration) = humantime::parse_duration(&duration_str)
        {
            return Ok(duration);
        }

        // 回退：尝试将值作为秒数（u64）解析
        let seconds = self
            .settings
            .get_int(key)
            .context(ConfigSnafu { category: "config" })?;
        Ok(Duration::from_secs(seconds as u64))
    }

    /// 读取字节大小配置值，支持 "10MB"、"1KB" 等人类可读格式，返回字节数。
    pub fn get_byte_size(&self, key: &str) -> Result<usize> {
        let value = self
            .settings
            .get_string(&self.get_key(key))
            .context(ConfigSnafu { category: "config" })?;
        let size = parse_size(value).context(ParseSizeSnafu { category: "config" })?;
        Ok(size as usize)
    }

    /// 创建具有指定前缀的子配置视图，用于隔离不同模块的配置命名空间。
    /// `prefix` 为空时返回当前配置的克隆。
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

        Config::new(&[toml_data], Some("TEST")).unwrap()
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
        let config = Config::new(&[""], None).unwrap();

        // Missing keys should return errors, not phantom defaults.
        assert!(config.get_int("any_key").is_err());
        assert!(config.get_bool("any_key").is_err());
        assert!(config.get_duration("any_key").is_err());
        assert!(config.get_byte_size("any_key").is_err());
    }

    #[test]
    fn test_environment_variable_override() {
        // This test would require setting environment variables
        // For now, we'll just test that the config can be created with env prefix
        let config = Config::new(&[""], Some("MYAPP")).unwrap();
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
        let config = Config::new(&[config1, config2], None).unwrap();
        assert_eq!(config.get_string("app_name").unwrap(), "config2"); // Overridden
        assert_eq!(config.get_int("port").unwrap(), 8080); // From config1
        assert_eq!(config.get_bool("debug").unwrap(), true); // From config2
    }
}
