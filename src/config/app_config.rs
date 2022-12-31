use config::{Config, File};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::env;

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
            return value
        }
        default_value.to_string()
    }
    fn get_int_value_from_env_first(&self, key: &str) -> i32 {
        convert_string_to_i32(self.get_value_from_env_first(key))
    }
    fn get_int_value_from_env_first_default(&self, key: &str, default_value: i32) -> i32 {
        let value = self.get_int_value_from_env_first(key);
        if value != 0 {
            return value
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
pub struct BasicConfig {
    // 监听地址
    pub listen: String,
    // 请求连接限制
    pub request_limit: i32,
}

pub fn must_new_basic_config() -> BasicConfig {
    let config = must_new_config().set_prefix("basic");
    BasicConfig {
        listen: config.get_value_from_env_first("listen"),
        request_limit: config.get_int_value_default("requestLimit", 5000),
    }
}

// redis 配置
pub struct RedisConfig {
    // redis连接地址
    pub uri: String,
}
pub fn must_new_redis_config() -> RedisConfig {
    let config = must_new_config().set_prefix("redis");
    // TODO validate
    RedisConfig {
        uri: config.get_value_from_env_first("uri"),
    }
}
