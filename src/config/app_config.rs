use config::{Config, File};
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use std::{collections::HashMap, env, fs, io::Write, path::PathBuf, time::Duration};
use substring::Substring;
use tempfile::TempDir;
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
    /// 从配置中获取对应的值(字符串)
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
    /// 从配置中获取对应的值(字符串)，
    /// 如果为空则使用默认值返回
    fn get_value_default(&self, key: &str, default_value: &str) -> String {
        let value = self.get_value(key);
        if !value.is_empty() {
            return value;
        }
        default_value.to_string()
    }
    /// 从配置中获取对应的值(i32)
    fn get_int_value(&self, key: &str) -> i32 {
        convert_string_to_i32(self.get_value(key))
    }
    /// 从配置中获取对应的值(i32)，
    /// 如果为0则使用默认值返回
    fn get_int_value_default(&self, key: &str, default_value: i32) -> i32 {
        let value = self.get_int_value(key);
        if value != 0 {
            return value;
        }
        default_value
    }
    /// 从配置中获取对应的值(bool)
    fn get_bool_value(&self, key: &str) -> bool {
        convert_string_to_bool(self.get_value(key))
    }
    /// 优先从env中获取配置的值，如果env中未配置则调用get_value获取
    fn get_value_from_env_first(&self, key: &str) -> String {
        let k = self.get_key(key);
        let mut env_key = k.replace('.', "_").to_uppercase();
        if !self.env_prefix.is_empty() {
            env_key = format!("{}_{env_key}", self.env_prefix);
        }
        if let Ok(value) = env::var(env_key) {
            return value;
        }
        self.get_value(key)
    }
    /// 使用get_value_from_env_first获取配置的值，
    /// 如果为空则使用默认值返回
    fn get_value_from_env_first_default(&self, key: &str, default_value: &str) -> String {
        let value = self.get_value_from_env_first(key);
        if !value.is_empty() {
            return value;
        }
        default_value.to_string()
    }
    /// 使用get_value_from_env_first获取配置的值，
    /// 并转换为 i32(转换失败则返回0)
    fn get_int_value_from_env_first(&self, key: &str) -> i32 {
        convert_string_to_i32(self.get_value_from_env_first(key))
    }
    /// 使用get_int_value_from_env_first获取配置的值，如果为0则返回默认值
    fn get_int_value_from_env_first_default(&self, key: &str, default_value: i32) -> i32 {
        let value = self.get_int_value_from_env_first(key);
        if value != 0 {
            return value;
        }
        default_value
    }
    /// 使用get_value_from_env_first获取配置的值，
    /// 并转换为bool(转换失败则返回false)
    fn get_bool_value_from_env_first(&self, key: &str) -> bool {
        convert_string_to_bool(self.get_value_from_env_first(key))
    }
}

fn write_data_to_temp_file(dir: &TempDir, name: &str) -> PathBuf {
    let default_path = dir.path().join(name);

    let mut file = fs::File::create(default_path.clone()).unwrap();

    file.write_all(&Configs::get(name).unwrap().data).unwrap();
    default_path
}

pub fn get_env() -> String {
    env::var("RUST_ENV").unwrap_or_else(|_| "dev".to_string())
}

fn must_new_config() -> &'static APPConfig {
    static APP_CONFIG: OnceCell<APPConfig> = OnceCell::new();
    APP_CONFIG.get_or_init(|| {
        let mode = get_env();

        // TODO config是否可直接使用字符串作为源
        let dir = tempfile::tempdir().unwrap();
        let default_file = write_data_to_temp_file(&dir, "default.yml");
        let current_file = write_data_to_temp_file(&dir, &format!("{mode}.yml"));

        let settings = Config::builder()
            .add_source(File::with_name(default_file.to_str().unwrap()))
            .add_source(File::with_name(current_file.to_str().unwrap()))
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
    let uri = config.get_value_from_env_first("uri");
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
                if let Ok(num) = value.parse::<u64>() {
                    redis_config.connection_timeout = Duration::from_millis(num);
                }
            }
            "wait_timeout" => {
                if let Ok(num) = value.parse::<u64>() {
                    redis_config.wait_timeout = Duration::from_millis(num);
                }
            }
            "recycle_timeout" => {
                if let Ok(num) = value.parse::<u64>() {
                    redis_config.recycle_timeout = Duration::from_millis(num);
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
