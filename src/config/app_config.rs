use config::{Config, File, FileFormat, FileSourceString};
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use std::{collections::HashMap, env, time::Duration};
use substring::Substring;
use url::Url;
use validator::Validate;

#[derive(RustEmbed)]
#[folder = "configs/"]
struct Configs;

#[derive(Debug, Clone, Default)]
pub struct APPConfig {
    // env变量的前缀
    env_prefix: String,
    // 应用配置的前缀
    prefix: String,
    // 应用配置信息
    settings: HashMap<String, HashMap<String, String>>,
}

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

impl APPConfig {
    /// 设置配置key的前缀
    fn set_prefix(&self, prefix: &str) -> APPConfig {
        let mut config = self.clone();
        config.prefix = prefix.to_string();
        config
    }
    /// 获取配置的key，若有设置前缀则添加前缀
    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        format!("{}.{key}", self.prefix)
    }
    /// 从配置中获取对应的值(字符串)，
    /// 如果为空则使用默认值返回
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
    /// 从配置中获取对应的值(i32)
    fn get_int(&self, key: &str, default_value: Option<i32>) -> i32 {
        let value = self.get(key, None);
        if !value.is_empty() {
            return convert_string_to_i32(value);
        }
        default_value.unwrap_or_default()
    }
    /// 从配置中获取duration
    fn get_duration(
        &self,
        key: &str,
        default_value: Option<Duration>,
    ) -> Result<Duration, humantime::DurationError> {
        let value = self.get(key, None);
        if !value.is_empty() {
            return humantime::parse_duration(&value);
        }
        Ok(default_value.unwrap_or_default())
    }
    /// 从配置中获取对应的值(bool)
    fn get_bool(&self, key: &str, default_value: Option<bool>) -> bool {
        let value = self.get(key, None);
        if !value.is_empty() {
            return convert_string_to_bool(value);
        }
        default_value.unwrap_or_default()
    }
    /// 优先从env中获取配置的值，如果env中未配置则调用get_value获取
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
    /// 使用get_value_from_env_first获取配置的值，
    /// 并转换为 i32(转换失败则返回0)
    fn get_int_from_env_first(&self, key: &str, default_value: Option<i32>) -> i32 {
        let value = self.get_from_env_first(key, None);
        if !value.is_empty() {
            return convert_string_to_i32(value);
        }
        default_value.unwrap_or_default()
    }
    /// 使用get_value_from_env_first获取配置的值，
    /// 并转换为bool(转换失败则返回false)
    fn get_bool_from_env_first(&self, key: &str, default_value: Option<bool>) -> bool {
        let value = self.get_from_env_first(key, None);
        if !value.is_empty() {
            return convert_string_to_bool(value);
        }
        default_value.unwrap_or_default()
    }
    /// 从配置中获取duration
    /// 如果获取失败则使用默认值返回
    fn get_duration_from_env_first(&self, key: &str, default_value: Option<Duration>) -> Duration {
        let value = self.get_from_env_first(key, None);
        let v = default_value.unwrap_or_default();
        if !value.is_empty() {
            return humantime::parse_duration(&value).unwrap_or(v);
        }
        v
    }
}

pub fn get_env() -> String {
    env::var("RUST_ENV").unwrap_or_else(|_| "dev".to_string())
}

fn must_new_source(name: &str) -> config::File<FileSourceString, FileFormat> {
    let str = std::string::String::from_utf8_lossy(&Configs::get(name).unwrap().data).to_string();
    File::from_str(str.as_str(), FileFormat::Yaml)
}

fn must_new_config() -> &'static APPConfig {
    static APP_CONFIG: OnceCell<APPConfig> = OnceCell::new();
    APP_CONFIG.get_or_init(|| {
        let mode = get_env();

        let settings = Config::builder()
            .add_source(must_new_source("default.yml"))
            .add_source(must_new_source(&format!("{mode}.yml")))
            .build()
            .unwrap()
            .try_deserialize::<HashMap<String, HashMap<String, String>>>()
            .unwrap();
        APPConfig {
            settings,
            ..Default::default()
        }
    })
}

// 基本配置
#[derive(Debug, Clone, Default, Validate)]
pub struct BasicConfig {
    // 监听地址
    #[validate(length(min = 1))]
    pub listen: String,
    // 请求连接限制
    #[validate(range(min = 0, max = 100000))]
    pub processing_limit: i32,
    // 超时
    pub timeout: Duration,
    #[validate(length(min = 1))]
    pub secret: String,
}

pub fn must_new_basic_config() -> BasicConfig {
    let config = must_new_config().set_prefix("basic");
    let timeout = config.get_duration_from_env_first("timeout", Some(Duration::from_secs(60)));
    let basic_config = BasicConfig {
        listen: config.get_from_env_first("listen", None),
        processing_limit: config.get_int_from_env_first("processing_limit", Some(5000)),
        timeout,
        secret: config.get_from_env_first("secret", None),
    };
    basic_config.validate().unwrap();
    basic_config
}

// redis 配置
#[derive(Debug, Clone, Default, Validate)]
pub struct RedisConfig {
    // redis连接地址
    #[validate(length(min = 1))]
    pub nodes: Vec<String>,
    // 连接池大小，默认为10
    pub pool_size: u32,
    // 连接超时，默认3秒
    pub connection_timeout: Duration,
    // 等待超时，默认3秒
    pub wait_timeout: Duration,
    // 复用超时，默认60秒
    pub recycle_timeout: Duration,
}
// 获取redis的配置
pub fn must_new_redis_config() -> RedisConfig {
    let config = must_new_config().set_prefix("redis");
    let uri = config.get_from_env_first("uri", None);
    let start = if let Some(index) = uri.find('@') {
        index + 1
    } else {
        uri.find("//").unwrap() + 2
    };

    let mut host = uri.substring(start, uri.len());
    if let Some(end) = host.find('/') {
        host = host.substring(0, end);
    }
    let mut nodes = vec![];
    for item in host.split(',') {
        nodes.push(uri.replace(host, item));
    }
    let info = Url::parse(&nodes[0]).unwrap();
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
    redis_config.validate().unwrap();
    redis_config
}

// session配置
#[derive(Debug, Clone, Default, Validate)]
pub struct SessionConfig {
    // session有效期
    #[validate(range(min = 60, max = 2592000))]
    pub ttl: usize,
    // session的secret，长度最少64
    #[validate(length(min = 64))]
    pub secret: String,
}
pub fn must_new_session_config() -> SessionConfig {
    let config = must_new_config().set_prefix("session");
    let ttl = config.get_duration_from_env_first("ttl", Some(Duration::from_secs(7 * 24 * 3600)));
    let session_config = SessionConfig {
        ttl: ttl.as_secs() as usize,
        secret: config.get_from_env_first("secret", None),
    };
    session_config.validate().unwrap();
    session_config
}

// 数据库配置
#[derive(Debug, Clone, Default, Validate)]
pub struct DatabaseConfig {
    pub origin_url: String,
    #[validate(length(min = 10))]
    pub url: String,
    #[validate(range(min = 2, max = 1000))]
    pub max_connections: u32,
    #[validate(range(min = 0, max = 10))]
    pub min_connections: u32,
    pub connect_timeout: Duration,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
}
pub fn must_new_database_config() -> DatabaseConfig {
    let config = must_new_config().set_prefix("database");
    let origin_url = config.get_from_env_first("url", None);
    let mut url = origin_url.clone();
    let info = Url::parse(&url).unwrap();
    let mut max_connections = 10;
    let mut min_connections = 2;
    let mut connect_timeout = Duration::from_secs(3);
    let mut acquire_timeout = Duration::from_secs(5);
    let mut idle_timeout = Duration::from_secs(60);

    if let Some(query) = info.query() {
        url = url.replace(query, "");
        for (key, value) in info.query_pairs() {
            match key.to_string().as_str() {
                "max_connections" => {
                    let value = convert_string_to_i32(value.to_string());
                    if value > 0 {
                        max_connections = value as u32;
                    }
                }
                "min_connections" => {
                    let value = convert_string_to_i32(value.to_string());
                    if value > 0 {
                        min_connections = value as u32;
                    }
                }
                "connect_timeout" => {
                    if let Ok(value) = humantime::parse_duration(&value) {
                        connect_timeout = value;
                    }
                }
                "acquire_timeout" => {
                    if let Ok(value) = humantime::parse_duration(&value) {
                        acquire_timeout = value;
                    }
                }
                "idle_timeout" => {
                    if let Ok(value) = humantime::parse_duration(&value) {
                        idle_timeout = value;
                    }
                }
                _ => {}
            }
        }
    }
    let database_config = DatabaseConfig {
        origin_url,
        url,
        max_connections,
        min_connections,
        connect_timeout,
        acquire_timeout,
        idle_timeout,
    };
    database_config.validate().unwrap();
    database_config
}
