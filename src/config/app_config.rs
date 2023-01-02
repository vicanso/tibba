use config::{Config, File};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::{env, time::Duration};
use url::Url;
use validator::Validate;

static APP_CONFIG: OnceCell<APPConfig> = OnceCell::new();

#[derive(Debug, Clone, Default)]
pub struct APPConfig {
    env_prefix: String,
    prefix: String,
    settings: HashMap<String, HashMap<String, String>>,
}

fn convert_string_to_i32(value: String) -> i32 {
    if let Ok(result) = value.parse::<i32>() {
        return result;
    }
    0
}

impl APPConfig {
    fn set_prefix(&self, prefix: &str) -> APPConfig {
        let mut config = self.clone();
        config.prefix = prefix.to_string();
        config
    }
    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        format!("{}.{}", self.prefix, key)
    }
    fn get_value(&self, key: &str) -> String {
        let k = self.get_key(key);
        let arr: Vec<&str> = k.split('.').collect();
        if arr.len() != 2 {
            return "".to_string();
        }
        if let Some(value) = self.settings.get(arr[0]) {
            if let Some(v) = value.get(arr[1]) {
                return v.clone();
            }
        }
        "".to_string()
    }
    fn get_value_default(&self, key: &str, default_value: &str) -> String {
        let value = self.get_value(key);
        if !value.is_empty() {
            return value;
        }
        default_value.to_string()
    }
    fn get_int_value(&self, key: &str) -> i32 {
        convert_string_to_i32(self.get_value(key))
    }
    fn get_int_value_default(&self, key: &str, default_value: i32) -> i32 {
        let value = self.get_int_value(key);
        if value != 0 {
            return value;
        }
        default_value
    }
    fn get_value_from_env_first(&self, key: &str) -> String {
        let k = self.get_key(key);
        let mut env_key = k.replace('.', "_").to_uppercase();
        if !self.env_prefix.is_empty() {
            env_key = format!("{}_{}", self.env_prefix, env_key);
        }
        if let Ok(value) = env::var(env_key) {
            return value;
        }
        self.get_value(key)
    }
    fn get_value_from_env_first_default(&self, key: &str, default_value: &str) -> String {
        let value = self.get_value_from_env_first(key);
        if !value.is_empty() {
            return value;
        }
        default_value.to_string()
    }
    fn get_int_value_from_env_first(&self, key: &str) -> i32 {
        convert_string_to_i32(self.get_value_from_env_first(key))
    }
    fn get_int_value_from_env_first_default(&self, key: &str, default_value: i32) -> i32 {
        let value = self.get_int_value_from_env_first(key);
        if value != 0 {
            return value;
        }
        default_value
    }
}

fn must_new_config() -> &'static APPConfig {
    APP_CONFIG.get_or_init(|| {
        let mode = env::var("RUST_ENV").unwrap_or_else(|_| "dev".to_string());
        let file = format!("configs/{}.yml", mode);
        let settings = Config::builder()
            .add_source(File::with_name("configs/default.yml"))
            .add_source(File::with_name(file.as_str()))
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
    pub request_limit: i32,
}

pub fn must_new_basic_config() -> BasicConfig {
    let config = must_new_config().set_prefix("basic");
    let basic_config = BasicConfig {
        listen: config.get_value_from_env_first("listen"),
        request_limit: config.get_int_value_default("requestLimit", 5000),
    };
    basic_config.validate().unwrap();
    basic_config
}

// redis 配置
#[derive(Debug, Clone, Default, Validate)]
pub struct RedisConfig {
    // redis连接地址
    #[validate(length(min = 1))]
    pub uri: String,
    // 连接池大小，默认为10
    pub pool_size: u32,
    // 空闲连接大小，默认为2
    pub idle: u32,
    // 连接超时，默认3秒
    pub connection_timeout: Duration,
}
pub fn must_new_redis_config() -> RedisConfig {
    let config = must_new_config().set_prefix("redis");
    let uri = config.get_value_from_env_first("uri");
    let info = Url::parse(uri.as_str()).unwrap();
    let mut redis_config = RedisConfig {
        uri: uri,
        pool_size: 10,
        idle: 2,
        connection_timeout: Duration::from_millis(3000),
    };
    for (key, value) in info.query_pairs() {
        match key.to_string().as_str() {
            "poolSize" => {
                if let Ok(num) = value.parse::<u32>() {
                    redis_config.pool_size = num;
                }
            }
            "idle" => {
                if let Ok(num) = value.parse::<u32>() {
                    redis_config.idle = num;
                }
            }
            "connectionTimeout" => {
                if let Ok(num) = value.parse::<u64>() {
                    redis_config.connection_timeout = Duration::from_millis(num);
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
    pub ttl: i32,
    // cookie名称
    #[validate(length(min = 1))]
    pub cookie: String,
    // session存储的key前缀
    #[validate(length(min = 1))]
    pub prefix: String,
    // session的secret，长度最少64
    #[validate(length(min = 64))]
    pub secret: String,
}
pub fn must_new_session_config() -> SessionConfig {
    let config = must_new_config().set_prefix("session");
    let session_config = SessionConfig {
        ttl: config.get_int_value_default("ttl", 7 * 24 * 3600),
        cookie: config.get_value_from_env_first_default("cookie", "tibba"),
        prefix: config.get_value_from_env_first_default("prefix", "ss:"),
        secret: config.get_value_from_env_first("secret"),
    };
    session_config.validate().unwrap();
    session_config
}
