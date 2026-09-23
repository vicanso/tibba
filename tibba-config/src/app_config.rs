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

use super::{
    BuildSnafu, ConflictingSecretSnafu, EmptySecretFileSnafu, Error, ParseSizeSnafu, ReadSnafu,
    SecretFileSnafu,
};
use config::{Config as RawConfig, Environment, File, FileFormat, Map};
use parse_size::parse_size;
use serde::Deserialize;
use snafu::{ResultExt, ensure};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

/// 环境变量层级分隔符。
///
/// 用 `__` 而非单 `_`：后者会把 `llm_api_key` 这类含下划线的字段名误拆成
/// `llm.api.key`，导致配置读不到。
const ENV_SEPARATOR: &str = "__";

/// 密钥文件环境变量的后缀，见 [`ConfigBuilder::with_env_prefix`]。
const SECRET_FILE_SUFFIX: &str = "_FILE";

/// 把 `X_FILE=<路径>` 展开成 `X=<文件内容>`。
///
/// # 为什么需要这个
/// 环境变量是**进程可见**的：`kubectl describe pod`、`docker inspect`、
/// `/proc/<pid>/environ` 都能看到，而且会被所有子进程继承。而 K8s Secret 与
/// Docker secret 的原生投递方式是**挂载成文件**。`*_FILE` 就是在这两者之间搭桥，
/// 也是 postgres / mysql / redis 官方镜像通行的约定。
///
/// # 匹配规则
/// 与 config-rs 的 `Environment` 保持一致：键名先转小写再比前缀，因此大小写
/// 不敏感。只处理带本配置前缀的变量——否则 `SSL_CERT_FILE` 这类系统环境变量
/// 会被当成密钥去读，把进程拦在启动前。
///
/// # 取值处理
/// 去掉**末尾**的换行（`\n` / `\r\n`）：`echo "secret" > file` 会带一个换行，
/// 而这类失误太常见。只去尾部，不做 trim——密钥中间与开头的空白都是有效内容。
fn expand_secret_files(
    raw: Map<String, String>,
    prefix: &str,
    separator: &str,
) -> Result<Map<String, String>> {
    let pattern = format!("{prefix}{separator}").to_lowercase();
    let suffix = SECRET_FILE_SUFFIX.to_lowercase();

    let mut expanded = raw.clone();
    for (file_key, path) in &raw {
        let lower = file_key.to_lowercase();
        if !lower.starts_with(&pattern) || !lower.ends_with(&suffix) {
            continue;
        }
        // 去掉 `_FILE` 得到真正的配置键
        let key = file_key[..file_key.len() - SECRET_FILE_SUFFIX.len()].to_string();
        // 同时设了 X 与 X_FILE：无法判断该用哪个，宁可起不来也不要用错密钥
        ensure!(
            !raw.contains_key(&key),
            ConflictingSecretSnafu {
                key,
                file_key: file_key.clone(),
            }
        );

        let content = std::fs::read_to_string(path).context(SecretFileSnafu {
            key: key.clone(),
            path: path.clone(),
        })?;
        let value = content.trim_end_matches(['\n', '\r']).to_string();
        ensure!(
            !value.is_empty(),
            EmptySecretFileSnafu {
                key: key.clone(),
                path: path.clone(),
            }
        );
        expanded.insert(key, value);
    }
    Ok(expanded)
}

/// 应用配置，封装底层 `config::Config`，提供命名空间与便捷读取方法。
///
/// 由 [`Config::builder`] 构造；环境变量前缀只在构造期用于装配 `Environment`
/// source，之后烘焙进 `settings`，不在实例上保留——这样可省一次 `String` clone
/// （在 [`Self::sub_config`] 内）并缩小 struct 体积。
#[derive(Clone, Default)]
pub struct Config {
    /// 子配置前缀，用于隔离不同模块的配置命名空间。
    prefix: String,
    /// 已烘焙的配置树，`Arc` 共享。
    ///
    /// 用 `Arc` 而非直接持有：`config::Config` 内部是完整的 `Map<String, Value>`
    /// 配置树，[`Config::sub_config`] 每调一次就要整棵深拷贝一遍。各模块在初始化
    /// 时普遍 `sub_config("redis")` / `sub_config("database")` 这样切命名空间，
    /// 换成引用计数后这些调用只增减一个计数。配置树构建后只读，共享安全。
    settings: Arc<RawConfig>,
}

/// 手写 `Debug` 而非 derive：`settings` 持有完整配置树，内含数据库口令、
/// 第三方 API key 等凭据。derive 出来的 `{:?}` 会把它们原样打进日志或 panic
/// 回溯——而 `Config` 常被塞进各模块的 struct 里，一次上层 derive 就会连带泄漏。
/// 这里只暴露命名空间前缀与条目数，凭据一律不出现。
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("prefix", &self.prefix)
            .field("settings", &"<redacted>")
            .finish()
    }
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
#[derive(Default)]
pub struct ConfigBuilder {
    /// TOML 源，按加入顺序生效，后者覆盖前者。
    sources: Vec<String>,
    /// 环境变量前缀；`None` 表示不挂载环境变量源。
    env_prefix: Option<String>,
    /// 环境变量层级分隔符，`None` 时取 [`ENV_SEPARATOR`]。
    env_separator: Option<String>,
}

/// 同 [`Config`] 的 `Debug`，且泄漏面更大：`sources` 存的是**原始 TOML 全文**，
/// derive 会把整份配置（含明文口令）打出来。构造期的错误处理最容易顺手
/// `{builder:?}`，故只输出源的数量与环境变量前缀，正文一律不出现。
impl std::fmt::Debug for ConfigBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigBuilder")
            .field(
                "sources",
                &format_args!("<{} redacted>", self.sources.len()),
            )
            .field("env_prefix", &self.env_prefix)
            .field("env_separator", &self.env_separator)
            .finish()
    }
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
    ///
    /// # 密钥从文件读取：`*_FILE`
    /// 任意配置项都可以改用 `<变量名>_FILE=<路径>` 的形式，值取自该文件的内容：
    ///
    /// ```text
    /// TIBBA_WEB__DATABASE__URI_FILE=/run/secrets/db_uri
    /// TIBBA_WEB__SESSION__SECRET_FILE=/run/secrets/session_secret
    /// ```
    ///
    /// 这不是可有可无的便利。环境变量是**进程可见**的——`kubectl describe pod`、
    /// `docker inspect`、`/proc/<pid>/environ` 都能读到，并且会被每一个子进程
    /// 继承；而 K8s Secret 与 Docker secret 的原生投递方式恰恰是挂载成文件。
    /// 同一套约定在 postgres / mysql / redis 官方镜像里已经用了很多年。
    ///
    /// 三条 fail-fast 规则（都属于部署失误，宁可起不来也不要带病运行）：
    /// - 文件读不出来 → [`Error::SecretFile`]（否则会静默回落到 TOML 里的占位口令）
    /// - 文件内容为空 → [`Error::EmptySecretFile`]
    /// - 同时设了 `X` 与 `X_FILE` → [`Error::ConflictingSecret`]
    ///
    /// 文件内容会去掉**末尾**换行（`echo "x" > f` 会带一个），其余字节原样保留。
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
            // 自行物化环境映射，而不是让 config-rs 去读：`*_FILE` 的展开必须发生在
            // config-rs 看到这批变量**之前**。
            //
            // 用 vars_os 而非 vars：后者遇到非 UTF-8 的环境变量会 panic，而那与本
            // 进程的配置毫不相干——跳过即可，不该让别人的脏环境变量掀翻启动。
            let raw: Map<String, String> = env_source.unwrap_or_else(|| {
                std::env::vars_os()
                    .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                    .collect()
            });
            let source = expand_secret_files(raw, &prefix, separator)?;

            let env = Environment::with_prefix(&prefix)
                .prefix_separator(separator)
                .separator(separator)
                // 空值视为未设置：`export XXX=` 不应把 TOML 里的值抹成空串
                .ignore_empty(true)
                .source(Some(source));
            builder = builder.add_source(env);
        }

        Ok(Config {
            prefix: String::new(),
            settings: Arc::new(builder.build().context(BuildSnafu)?),
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
    ///
    /// 两种格式都解析不了时，报的错会带上**键名与实际取值**。此前的实现是
    /// 「humantime 失败就去试 `get_int`」，于是 `timeout = "abc"` 报出来的是
    /// 一句「期望整数」——把排查方向引向类型问题，而真正的原因是时长格式写错了。
    pub fn get_duration(&self, key: &str) -> Result<Duration> {
        let full_key = self.get_key(key);
        // 统一按字符串取：config-rs 会把整数值也转成字符串，故一次读取即可覆盖
        // `timeout = "60s"` 与 `timeout = 120` 两种写法，无需读两遍。
        // 键不存在 / 类型无法转字符串时，这里的 ReadSnafu 就是准确的错误。
        let raw = self.settings.get_string(&full_key).context(ReadSnafu)?;

        if let Ok(duration) = humantime::parse_duration(&raw) {
            return Ok(duration);
        }
        // 回退：纯**非负**整数视为秒数。
        //
        // 负数不再悄悄钳成 0：对超时类配置，0 往往意味着「不超时」或「立即过期」，
        // 两种都和写 `-1` 的人想要的东西毫无关系。解析成 u64 让负数自然落到下面
        // 的 InvalidDuration，报错里带着键名与原值，比一个静默的 0 好排查得多。
        if let Ok(seconds) = raw.trim().parse::<u64>() {
            return Ok(Duration::from_secs(seconds));
        }

        Err(Error::InvalidDuration {
            key: full_key.into_owned(),
            value: raw,
        })
    }

    /// 读取字节大小配置值，支持 "10MB"、"1KB" 等人类可读格式，返回字节数。
    pub fn get_byte_size(&self, key: &str) -> Result<usize> {
        let value = self
            .settings
            .get_string(&self.get_key(key))
            .context(ReadSnafu)?;
        let size = parse_size(value).context(ParseSizeSnafu)?;
        // parse_size 返回 u64；32 位目标上 `as usize` 会静默截断成一个小得多的值
        // （`8GB` → 0），这里宁可报错
        usize::try_from(size).map_err(|_| Error::InvalidByteSize {
            key: self.get_key(key).into_owned(),
            value: size,
        })
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
            // 引用计数 +1，不复制配置树
            settings: Arc::clone(&self.settings),
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

    /// 时长格式写错时，错误必须指向「时长格式」并带上键名与实际取值，
    /// 而不是像此前那样回退去试整数、最终报一句误导性的类型错误。
    #[test]
    fn invalid_duration_error_names_key_and_value() {
        let config = Config::builder()
            .add_toml(r#"timeout = "abc""#)
            .build()
            .unwrap();
        let err = config.get_duration("timeout").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid duration"),
            "错误应指向时长格式: {msg}"
        );
        assert!(msg.contains("timeout"), "错误应带上键名: {msg}");
        assert!(msg.contains("abc"), "错误应带上实际取值: {msg}");

        // 子配置下键名应是完整路径，便于直接定位到配置文件里的位置
        let err = config
            .sub_config("svc")
            .get_duration("nope")
            .unwrap_err()
            .to_string();
        assert!(err.contains("svc.nope"), "子配置应报完整键路径: {err}");
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

    /// `sub_config` / `clone` 必须共享同一棵配置树，而不是深拷贝。
    ///
    /// 退回 `settings: RawConfig` 时每次 sub_config 都会整棵复制一遍——各模块
    /// 初始化时普遍要切命名空间，这条路径值得钉死。
    #[test]
    fn sub_config_shares_tree_instead_of_deep_copying() {
        let config = create_test_config();
        let before = Arc::strong_count(&config.settings);

        let db = config.sub_config("database");
        let cache = config.sub_config("cache");
        let nested = db.sub_config("connection");
        let cloned = config.clone();

        // 四个派生实例都指向同一份 Arc
        assert!(Arc::ptr_eq(&config.settings, &db.settings));
        assert!(Arc::ptr_eq(&config.settings, &cache.settings));
        assert!(Arc::ptr_eq(&config.settings, &nested.settings));
        assert!(Arc::ptr_eq(&config.settings, &cloned.settings));
        assert_eq!(Arc::strong_count(&config.settings), before + 4);

        // 共享不影响各自的命名空间隔离
        assert_eq!(db.get_string("host").unwrap(), "localhost");
        assert_eq!(cache.get_bool("enabled").unwrap(), true);
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

    /// 在临时目录里写一个密钥文件，返回其路径。
    fn write_secret(dir: &std::path::Path, name: &str, content: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, content).expect("写密钥文件");
        path.to_string_lossy().into_owned()
    }

    /// 每个用例一个独立目录，避免并行测试互相踩。
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tibba-config-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        dir
    }

    /// `*_FILE` 把文件内容取成配置值，并去掉尾部换行。
    ///
    /// 这条是给 K8s / Docker secret 用的：它们把密钥挂成文件，而环境变量
    /// 在 `kubectl describe pod` / `/proc/<pid>/environ` 里是明文可见的。
    #[test]
    fn secret_file_supplies_value_and_trims_trailing_newline() {
        let dir = temp_dir("basic");
        // `echo "x" > f` 会带一个尾换行，这是最常见的失误
        let path = write_secret(
            &dir,
            "session_secret",
            "s3cr3t
",
        );
        let uri_path = write_secret(
            &dir,
            "db_uri",
            "postgres://u:p@h/db
",
        );

        let config = Config::builder()
            .add_toml(
                r#"
                [session]
                secret = "from-toml"
                [database]
                uri = "from-toml"
                "#,
            )
            .with_env_prefix("MYAPP")
            .build_with_env(Some(env_map(&[
                ("MYAPP__SESSION__SECRET_FILE", &path),
                ("MYAPP__DATABASE__URI_FILE", &uri_path),
            ])))
            .expect("构建配置");

        assert_eq!(config.get_string("session.secret").unwrap(), "s3cr3t");
        assert_eq!(
            config.get_string("database.uri").unwrap(),
            "postgres://u:p@h/db"
        );
    }

    /// 只去尾部换行，密钥中间与开头的空白是有效内容，不得 trim 掉。
    #[test]
    fn secret_file_preserves_inner_and_leading_whitespace() {
        let dir = temp_dir("whitespace");
        let path = write_secret(
            &dir,
            "pw",
            "  a b	c  
",
        );
        let config = Config::builder()
            .with_env_prefix("MYAPP")
            .build_with_env(Some(env_map(&[("MYAPP__PW_FILE", &path)])))
            .expect("构建配置");
        assert_eq!(config.get_string("pw").unwrap(), "  a b	c  ");
    }

    /// **fail fast**：文件读不出来必须报错。
    ///
    /// 若放行，`ignore_empty` 会把它当作未设置，于是静默回落到 TOML 里的
    /// 占位口令——带着一个人畜无害的默认密钥跑起来，比起不来危险得多。
    #[test]
    fn missing_secret_file_fails_startup() {
        let err = Config::builder()
            .add_toml(r#"secret = "from-toml""#)
            .with_env_prefix("MYAPP")
            .build_with_env(Some(env_map(&[(
                "MYAPP__SECRET_FILE",
                "/nonexistent/tibba/secret",
            )])))
            .expect_err("读不到密钥文件应当失败");
        assert!(matches!(err, Error::SecretFile { .. }), "{err}");
        // 错误信息里只能有键名与路径，不能有内容
        assert!(err.to_string().contains("MYAPP__SECRET"));
    }

    /// 空密钥文件同样是部署失误（secret 没挂上 / 挂错路径）。
    #[test]
    fn empty_secret_file_fails_startup() {
        let dir = temp_dir("empty");
        let path = write_secret(
            &dir, "empty", "
",
        );
        let err = Config::builder()
            .with_env_prefix("MYAPP")
            .build_with_env(Some(env_map(&[("MYAPP__SECRET_FILE", &path)])))
            .expect_err("空密钥文件应当失败");
        assert!(matches!(err, Error::EmptySecretFile { .. }), "{err}");
    }

    /// 同时给 `X` 与 `X_FILE` 无法判断该用哪个，直接拒绝——
    /// 静默挑一个就可能用错密钥，而这种错极难排查。
    #[test]
    fn conflicting_secret_sources_are_rejected() {
        let dir = temp_dir("conflict");
        let path = write_secret(&dir, "s", "from-file");
        let err = Config::builder()
            .with_env_prefix("MYAPP")
            .build_with_env(Some(env_map(&[
                ("MYAPP__SECRET", "from-env"),
                ("MYAPP__SECRET_FILE", &path),
            ])))
            .expect_err("同时设置两者应当被拒绝");
        assert!(matches!(err, Error::ConflictingSecret { .. }), "{err}");
    }

    /// **关键**：不带本配置前缀的 `*_FILE` 必须被无视。
    ///
    /// `SSL_CERT_FILE`、`GIT_CONFIG_FILE` 这类系统环境变量到处都是，
    /// 若不按前缀过滤，它们会被当成密钥去读，把进程直接拦在启动前。
    #[test]
    fn unrelated_file_env_vars_are_ignored() {
        let config = Config::builder()
            .add_toml(r#"ok = "yes""#)
            .with_env_prefix("MYAPP")
            .build_with_env(Some(env_map(&[
                ("SSL_CERT_FILE", "/nonexistent/ca.pem"),
                ("OTHER__SECRET_FILE", "/nonexistent/other"),
            ])))
            .expect("无关的 *_FILE 不应影响启动");
        assert_eq!(config.get_string("ok").unwrap(), "yes");
    }

    /// 键名匹配大小写不敏感，与 config-rs 的 Environment 规则一致。
    #[test]
    fn secret_file_matching_is_case_insensitive() {
        let dir = temp_dir("case");
        let path = write_secret(&dir, "lower", "v");
        let config = Config::builder()
            .with_env_prefix("MYAPP")
            .build_with_env(Some(env_map(&[("myapp__secret_file", &path)])))
            .expect("构建配置");
        assert_eq!(config.get_string("secret").unwrap(), "v");
    }

    /// 负数时长必须报错，而不是静默变成 0（「不超时」或「立即过期」）。
    #[test]
    fn negative_duration_is_rejected_not_clamped() {
        let config = Config::builder()
            .add_toml("timeout = -1\nok = 30")
            .build()
            .expect("构建配置");
        let err = config
            .get_duration("timeout")
            .expect_err("负数时长应当报错");
        assert!(matches!(err, Error::InvalidDuration { .. }), "{err}");
        assert_eq!(config.get_duration("ok").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn debug_does_not_leak_secrets() {
        // Config / ConfigBuilder 的 `{:?}` 绝不能吐出配置正文——这是防止口令
        // 随日志或 panic 回溯外泄的守卫，回归到 derive(Debug) 时本例会失败。
        let toml = r#"
            [database]
            password = "super-secret-pw"

            [email]
            api_key = "k-should-never-be-logged"
        "#;
        let builder = Config::builder().add_toml(toml).with_env_prefix("MYAPP");
        let builder_debug = format!("{builder:?}");
        assert!(!builder_debug.contains("super-secret-pw"));
        assert!(!builder_debug.contains("k-should-never-be-logged"));
        // 非敏感的结构信息仍应可见，便于排查
        assert!(builder_debug.contains("MYAPP"));

        let config = builder.build().unwrap();
        let config_debug = format!("{:?}", config.sub_config("database"));
        assert!(!config_debug.contains("super-secret-pw"));
        assert!(!config_debug.contains("k-should-never-be-logged"));
        assert!(config_debug.contains("database"));

        // 脱敏只针对 Debug，正常读取路径不受影响
        assert_eq!(
            config.get_string("database.password").unwrap(),
            "super-secret-pw"
        );
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
