// Copyright 2026 Tree xie.
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

use super::{BuildSnafu, Error, ParseSizeSnafu, ReadSnafu};
use config::{Config as RawConfig, Environment, File, FileFormat, Map};
use parse_size::parse_size;
use serde::Deserialize;
use snafu::ResultExt;
use std::borrow::Cow;
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

/// 环境变量层级分隔符。
///
/// 用 `__` 而非单 `_`：后者会把 `llm_api_key` 这类含下划线的字段名误拆成
/// `llm.api.key`，导致配置读不到。
const ENV_SEPARATOR: &str = "__";

/// 应用配置，封装底层 `config::Config`，提供命名空间与便捷读取方法。
///
/// 由 [`Config::builder`] 构造；环境变量前缀只在构造期用于装配 `Environment`
/// source，之后烘焙进 `settings`，不在实例上保留——这样可省一次 `String` clone
/// （在 [`Self::sub_config`] 内）并缩小 struct 体积。
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// 子配置前缀，用于隔离不同模块的配置命名空间。
    prefix: String,
    settings: RawConfig,
}

/// [`Config`] 的构造器：TOML 源可追加多份，环境变量覆盖为可选项。
///
/// ```ignore
/// let config = Config::builder()
///     .add_toml(default_toml)
///     .add_toml(env_toml)
///     .with_env_prefix("TIBBA_WEB")
///     .build()?;
/// ```
#[derive(Debug, Default)]
pub struct ConfigBuilder {
    /// TOML 源，按加入顺序生效，后者覆盖前者。
    sources: Vec<String>,
    /// 环境变量前缀；`None` 表示不挂载环境变量源。
    env_prefix: Option<String>,
    /// 环境变量层级分隔符，`None` 时取 [`ENV_SEPARATOR`]。
    env_separator: Option<String>,
}

impl ConfigBuilder {
    /// 追加一份 TOML 配置源；后加入的覆盖先加入的，空串忽略。
    #[must_use]
    pub fn add_toml(mut self, data: impl Into<String>) -> Self {
        let data = data.into();
        if !data.is_empty() {
            self.sources.push(data);
        }
        self
    }

    /// 设置环境变量前缀，优先级高于所有 TOML 源。
    ///
    /// 例如前缀 `TIBBA_WEB` 时，`TIBBA_WEB__DATABASE__HOST` 覆盖 `database.host`，
    /// `TIBBA_WEB__EMAIL__API_KEY` 覆盖 `email.api_key`（单 `_` 是字段名的一部分）。
    ///
    /// 不调用本方法则**完全不挂载**环境变量源。空串等同于不设置。
    #[must_use]
    pub fn with_env_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.env_prefix = Some(prefix.into());
        self
    }

    /// 自定义环境变量层级分隔符，默认 [`ENV_SEPARATOR`]（`__`）。
    #[must_use]
    pub fn with_env_separator(mut self, separator: impl Into<String>) -> Self {
        self.env_separator = Some(separator.into());
        self
    }

    /// 构建配置。
    pub fn build(self) -> Result<Config> {
        self.build_with_env(None)
    }

    /// 内部构建入口。`env_source` 为 `Some` 时以给定映射替代进程环境变量，
    /// 供单测使用——`std::env::set_var` 在多线程 test binary 中与其它线程读环境变量
    /// 存在竞态（Rust 2024 已将其标记为 `unsafe`），这里绕开。
    fn build_with_env(self, env_source: Option<Map<String, String>>) -> Result<Config> {
        let mut builder = RawConfig::builder();
        for data in &self.sources {
            builder = builder.add_source(File::from_str(data, FileFormat::Toml));
        }

        // 空前缀不能直接透传：config-rs 会把 prefix_pattern 算成分隔符本身（`__`），
        // 等于要求所有环境变量以 `__` 开头，覆盖能力静默失效。故空前缀直接不挂该源。
        if let Some(prefix) = self.env_prefix.filter(|p| !p.is_empty()) {
            let separator = self.env_separator.as_deref().unwrap_or(ENV_SEPARATOR);
            let mut env = Environment::with_prefix(&prefix)
                .prefix_separator(separator)
                .separator(separator)
                // 空值视为未设置：`export XXX=` 不应把 TOML 里的值抹成空串
                .ignore_empty(true);
            if env_source.is_some() {
                env = env.source(env_source);
            }
            builder = builder.add_source(env);
        }

        Ok(Config {
            prefix: String::new(),
            settings: builder.build().context(BuildSnafu)?,
        })
    }
}

impl Config {
    /// 创建配置构造器，见 [`ConfigBuilder`]。
    #[must_use]
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    /// 将前缀与键名拼接为完整的配置键路径。
    /// 仅在两者都非空时分配新 `String`，否则借用现有切片。
    fn get_key<'a>(&'a self, key: &'a str) -> Cow<'a, str> {
        match (self.prefix.is_empty(), key.is_empty()) {
            (true, _) => Cow::Borrowed(key),
            (false, true) => Cow::Borrowed(&self.prefix),
            (false, false) => Cow::Owned(format!("{}.{}", self.prefix, key)),
        }
    }

    /// 将当前命名空间下的配置整体反序列化为指定类型。
    pub fn try_deserialize<'de, T: Deserialize<'de>>(&self) -> Result<T> {
        self.settings.get(&self.get_key("")).context(ReadSnafu)
    }

    /// 读取任意可反序列化类型的配置值。
    pub fn get<'de, T: Deserialize<'de>>(&self, key: &str) -> Result<T> {
        self.settings.get(&self.get_key(key)).context(ReadSnafu)
    }

    /// 读取字符串类型的配置值。
    pub fn get_string(&self, key: &str) -> Result<String> {
        self.settings
            .get_string(&self.get_key(key))
            .context(ReadSnafu)
    }

    /// 读取 i64 类型的整数配置值。
    pub fn get_int(&self, key: &str) -> Result<i64> {
        self.settings.get_int(&self.get_key(key)).context(ReadSnafu)
    }

    /// 读取 f64 类型的浮点数配置值。
    pub fn get_float(&self, key: &str) -> Result<f64> {
        self.settings
            .get_float(&self.get_key(key))
            .context(ReadSnafu)
    }

    /// 读取布尔类型的配置值。
    pub fn get_bool(&self, key: &str) -> Result<bool> {
        self.settings
            .get_bool(&self.get_key(key))
            .context(ReadSnafu)
    }

    /// 读取时间长度配置值。
    /// 优先解析人类可读格式（如 "10s"、"1h"），失败则回退为纯数字（秒）。
    pub fn get_duration(&self, key: &str) -> Result<Duration> {
        let full_key = self.get_key(key);
        if let Ok(duration_str) = self.settings.get_string(&full_key)
            && let Ok(duration) = humantime::parse_duration(&duration_str)
        {
            return Ok(duration);
        }

        // 回退：尝试将值作为秒数解析；负数视为 0，避免 i64→u64 回绕成天文数字时长
        let seconds = self.settings.get_int(&full_key).context(ReadSnafu)?;
        Ok(Duration::from_secs(seconds.max(0) as u64))
    }

    /// 读取字节大小配置值，支持 "10MB"、"1KB" 等人类可读格式，返回字节数。
    pub fn get_byte_size(&self, key: &str) -> Result<usize> {
        let value = self
            .settings
            .get_string(&self.get_key(key))
            .context(ReadSnafu)?;
        let size = parse_size(value).context(ParseSizeSnafu)?;
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

        Config::builder()
            .add_toml(toml_data)
            .with_env_prefix("TEST")
            .build()
            .unwrap()
    }

    #[test]
    fn test_config_creation() {
        let config = create_test_config();
        // env_prefix 已烘焙进 settings，不再保留字段，仅断言 prefix 默认空
        assert_eq!(config.prefix, "");
    }

    #[test]
    fn test_get_str() {
        let config = create_test_config();
        assert_eq!(config.get_string("app_name").unwrap(), "test_app");
    }

    #[test]
    fn test_get_int() {
        let config = create_test_config();
        assert_eq!(config.get_int("port").unwrap(), 8080);
        assert_eq!(config.get_int("negative_number").unwrap(), -42);
    }

    #[test]
    fn test_get_bool() {
        let config = create_test_config();
        assert_eq!(config.get_bool("debug").unwrap(), true);
        assert_eq!(config.get_bool("production").unwrap(), false);
    }

    #[test]
    fn test_get_duration_human_readable() {
        let config = create_test_config();
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
        assert_eq!(
            config.get_duration("numeric_timeout").unwrap(),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn test_get_duration_negative_clamped() {
        // 负秒数应被钳为 0，避免 i64→u64 回绕成天文数字
        let toml = r#"backwards = -10"#;
        let config = Config::builder().add_toml(toml).build().unwrap();
        assert_eq!(config.get_duration("backwards").unwrap(), Duration::ZERO);
    }

    #[test]
    fn test_get_byte_size() {
        let config = create_test_config();
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

        let db_config = config.sub_config("database");
        assert_eq!(db_config.prefix, "database");
        assert_eq!(db_config.get_string("host").unwrap(), "localhost");
        assert_eq!(db_config.get_int("port").unwrap(), 5432);
        assert_eq!(
            db_config.get_duration("timeout").unwrap(),
            Duration::from_secs(30)
        );

        let cache_config = config.sub_config("cache");
        assert_eq!(cache_config.prefix, "cache");
        assert_eq!(cache_config.get_bool("enabled").unwrap(), true);
        assert_eq!(
            cache_config.get_duration("ttl").unwrap(),
            Duration::from_secs(600)
        );
        assert_eq!(cache_config.get_byte_size("max_size").unwrap(), 100_000_000);
    }

    #[test]
    fn test_nested_sub_config() {
        let config = create_test_config();
        let db_config = config.sub_config("database");
        let nested_config = db_config.sub_config("connection");
        assert_eq!(nested_config.prefix, "database.connection");
    }

    #[test]
    fn test_get_generic() {
        let config = create_test_config();
        assert_eq!(config.get::<String>("app_name").unwrap(), "test_app");
        assert_eq!(config.get::<i64>("port").unwrap(), 8080);
        assert_eq!(config.get::<bool>("debug").unwrap(), true);
        assert!(config.get::<String>("non_existent").is_err());
    }

    #[test]
    fn test_empty_config() {
        // 空串源被忽略，等价于没有任何配置源
        let config = Config::builder().add_toml("").build().unwrap();
        assert!(config.get_int("any_key").is_err());
        assert!(config.get_bool("any_key").is_err());
        assert!(config.get_duration("any_key").is_err());
        assert!(config.get_byte_size("any_key").is_err());
    }

    /// 构造假环境变量映射，避免 `std::env::set_var` 的多线程竞态。
    fn env_map(pairs: &[(&str, &str)]) -> Map<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn test_environment_variable_override() {
        let toml = r#"
            [database]
            host = "localhost"
            port = 5432

            [email]
            api_key = "from-toml"
        "#;
        let config = Config::builder()
            .add_toml(toml)
            .with_env_prefix("MYAPP")
            .build_with_env(Some(env_map(&[
                ("MYAPP__DATABASE__HOST", "from-env"),
                // 单 `_` 是字段名的一部分，不会被拆成 email.api.key
                ("MYAPP__EMAIL__API_KEY", "k-123456"),
                // 空值视为未设置，不得把 TOML 里的 5432 抹掉
                ("MYAPP__DATABASE__PORT", ""),
                // 前缀不匹配的变量必须被忽略
                ("OTHER__DATABASE__HOST", "should-be-ignored"),
            ])))
            .unwrap();
        assert_eq!(config.get_string("database.host").unwrap(), "from-env");
        assert_eq!(config.get_string("email.api_key").unwrap(), "k-123456");
        assert_eq!(config.get_int("database.port").unwrap(), 5432);
    }

    #[test]
    fn test_no_env_prefix_mounts_no_env_source() {
        // 不调 with_env_prefix 则完全不挂环境变量源。
        // 旧实现把 None 透传成 with_prefix("")，prefix_pattern 会变成 "__"，
        // 于是 `__NOPREFIX` 反而能覆盖配置——本例正是那个回归的守卫。
        let config = Config::builder()
            .add_toml(r#"noprefix = "from-toml""#)
            .build_with_env(Some(env_map(&[("__NOPREFIX", "leaked")])))
            .unwrap();
        assert_eq!(config.get_string("noprefix").unwrap(), "from-toml");
    }

    #[test]
    fn test_custom_env_separator() {
        let config = Config::builder()
            .add_toml(
                r#"[database]
                host = "localhost""#,
            )
            .with_env_prefix("MYAPP")
            .with_env_separator("_")
            .build_with_env(Some(env_map(&[("MYAPP_DATABASE_HOST", "from-env")])))
            .unwrap();
        assert_eq!(config.get_string("database.host").unwrap(), "from-env");
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
        let config = Config::builder()
            .add_toml(config1)
            .add_toml(config2)
            .build()
            .unwrap();
        assert_eq!(config.get_string("app_name").unwrap(), "config2");
        assert_eq!(config.get_int("port").unwrap(), 8080);
        assert_eq!(config.get_bool("debug").unwrap(), true);
    }
}
