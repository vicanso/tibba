use crate::config::must_new_redis_config;
use crate::error::HTTPError;
use crate::util::{snappy_decode, snappy_encode, zstd_decode, zstd_encode, CompressError};
use deadpool_redis::redis::{cmd, pipe};
use deadpool_redis::{Connection, Manager, Pool, PoolConfig, Runtime};
use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use redis::Client;
use serde::{de::DeserializeOwned, Serialize};
use snafu::{ResultExt, Snafu};
use std::time::Duration;

// 如果要支持cluster，需要使用deadpool_redis_cluster
pub fn must_new_redis_client() -> Client {
    let config = must_new_redis_config();
    Client::open(config.nodes[0].as_str()).unwrap()
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Redis {category}: {source}"))]
    Redis {
        category: String,
        source: deadpool_redis::redis::RedisError,
    },
    #[snafu(display("Redis pool: {source}"))]
    Pool { source: deadpool_redis::PoolError },
    #[snafu(display("Json {category}: {source}"))]
    Json {
        category: String,
        source: serde_json::Error,
    },
    #[snafu(display("{source}"))]
    Compress { source: CompressError },
}

impl From<CompressError> for Error {
    fn from(value: CompressError) -> Self {
        Error::Compress { source: value }
    }
}

impl From<Error> for HTTPError {
    fn from(err: Error) -> Self {
        // 对于部分error单独转换
        match err {
            Error::Redis { category, source } => {
                HTTPError::new_with_category(&source.to_string(), &category)
            }
            Error::Pool { source } => {
                HTTPError::new_with_category(&source.to_string(), "redis_pool")
            }
            Error::Json { category, source } => {
                HTTPError::new_with_category(&source.to_string(), &category)
            }
            Error::Compress { source } => source.into(),
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

static REDIS_POOL: Lazy<Pool> = Lazy::new(|| {
    let config = must_new_redis_config();
    let p = Pool::builder(Manager::new(config.nodes[0].as_str()).unwrap());
    p.config(PoolConfig {
        max_size: config.pool_size as usize,
        timeouts: deadpool_redis::Timeouts {
            wait: Some(config.wait_timeout),
            create: Some(config.connection_timeout),
            recycle: Some(config.recycle_timeout),
        },
    })
    .runtime(Runtime::Tokio1)
    .build()
    .unwrap()
});

pub async fn get_redis_conn() -> Result<Connection> {
    REDIS_POOL.get().await.context(PoolSnafu {})
}

pub async fn redis_ping() -> Result<String> {
    let mut conn = get_redis_conn().await?;
    cmd("PING")
        .query_async::<Connection, String>(&mut conn)
        .await
        .context(RedisSnafu { category: "ping" })
}

#[derive(Default, Clone, Debug)]
pub struct RedisCache {
    ttl: Duration,
    prefix: String,
}

/// 获取默认的redis缓存，基于redis pool并设置默认的ttl
pub fn get_default_redis_cache() -> &'static RedisCache {
    static DEFAULT_REDIS_CACHE: OnceCell<RedisCache> = OnceCell::new();

    DEFAULT_REDIS_CACHE.get_or_init(|| -> RedisCache { RedisCache::new() })
}

impl RedisCache {
    fn get_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            return key.to_string();
        }
        self.prefix.to_string() + key
    }
    /// 使用默认的ttl初始化redis缓存实例
    pub fn new() -> RedisCache {
        Self::new_with_ttl(Duration::from_secs(5 * 60))
    }
    /// 初始化redis缓存实例，并指定默认的ttl
    pub fn new_with_ttl(ttl: Duration) -> RedisCache {
        RedisCache {
            ttl,
            ..Default::default()
        }
    }
    /// 初始化redis缓存实例，指定ttl以及prefix
    pub fn new_with_ttl_prefix(ttl: Duration, prefix: String) -> RedisCache {
        RedisCache { ttl, prefix }
    }
    /// 尝试锁定key，时长为ttl，若未指定时长则使用默认时长
    /// 如果成功则返回true，否则返回false。
    /// 主要用于多实例并发限制。
    pub async fn lock(&self, key: &str, ttl: Option<Duration>) -> Result<bool> {
        let mut conn = get_redis_conn().await?;
        let k = self.get_key(key);

        let result = cmd("SET")
            .arg(&k)
            .arg(true)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "lock" })?;
        Ok(result)
    }
    /// 从redis中删除key
    pub async fn del(&self, key: &str) -> Result<()> {
        let mut conn = get_redis_conn().await?;
        let k = self.get_key(key);

        cmd("DEL")
            .arg(&k)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "del" })?;
        Ok(())
    }
    /// 增加redis中key所对应的值，如果ttl未指定则使用默认值，
    /// 需要注意此ttl仅在首次时设置。
    pub async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        let mut conn = get_redis_conn().await?;
        let k = self.get_key(key);
        let (_, count) = pipe()
            .cmd("SET")
            .arg(&k)
            .arg(0)
            .arg("NX")
            .arg("EX")
            .arg(ttl.unwrap_or(self.ttl).as_secs())
            .cmd("INCRBY")
            .arg(&k)
            .arg(delta)
            .query_async::<Connection, (bool, i64)>(&mut conn)
            .await
            .context(RedisSnafu { category: "incr" })?;
        Ok(count)
    }
    /// 将数据设置至redis中，如果未设置ttl则使用默认值
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let mut conn = get_redis_conn().await?;

        let seconds = ttl.unwrap_or(self.ttl).as_secs();
        cmd("SETEX")
            .arg(key)
            .arg(seconds)
            .arg(value)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu {
                category: "set_bytes",
            })?;
        Ok(())
    }
    /// 将数据设置至redis中，如果未设置ttl则使用默认值
    pub async fn set_bytes(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let k = self.get_key(key);
        self.set(&k, value, ttl).await
    }
    /// 从redis中获取数据
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let mut conn = get_redis_conn().await?;
        let result = cmd("GET")
            .arg(key)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu {
                category: "get_bytes",
            })?;

        Ok(result)
    }
    /// 从redis中获取数据
    pub async fn get_bytes(&self, key: &str) -> Result<Vec<u8>> {
        let k = self.get_key(key);
        self.get(&k).await
    }
    /// 将struct转换为json后设置至redis中，若未指定ttl则使用默认值
    pub async fn set_struct<T>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).context(JsonSnafu {
            category: "set_struct",
        })?;
        let k = self.get_key(key);
        self.set(&k, value, ttl).await?;
        Ok(())
    }
    /// 从redis中获取数据并转换为struct，如果缓存中无数据则使用struct的默认值返回
    pub async fn get_struct<'a, T>(&self, key: &str) -> Result<T>
    where
        T: Default + DeserializeOwned,
    {
        let k = self.get_key(key);
        let buf = self.get(&k).await?;

        if buf.is_empty() {
            return Ok(T::default());
        }

        let deserializer = &mut serde_json::Deserializer::from_slice(&buf);
        T::deserialize(deserializer).context(JsonSnafu {
            category: "get_struct",
        })
    }
    /// 返回该key在redis中的有效期
    pub async fn ttl(&self, key: &str) -> Result<i32> {
        let mut conn = get_redis_conn().await?;
        let k = self.get_key(key);
        let result = cmd("TTL")
            .arg(&k)
            .query_async(&mut conn)
            .await
            .context(RedisSnafu { category: "ttl" })?;
        Ok(result)
    }
    /// 获取后并删除该key在redis中的值，用于仅获取一次的场景
    pub async fn get_del(&self, key: &str) -> Result<Vec<u8>> {
        let k = self.get_key(key);
        let mut conn = get_redis_conn().await?;
        let (value, _) = pipe()
            .cmd("GET")
            .arg(&k)
            .cmd("DEL")
            .arg(&k)
            .query_async::<Connection, (Vec<u8>, bool)>(&mut conn)
            .await
            .context(RedisSnafu {
                category: "get_del",
            })?;
        Ok(value)
    }
    /// 将struct转换为json后使用snappy压缩，
    /// 再将压缩后的数据设置至redis中，若未指定ttl，
    /// 则使用默认的有效期
    pub async fn set_struct_snappy<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).context(JsonSnafu {
            category: "set_struct_snappy",
        })?;
        let buf = snappy_encode(&value)?;
        let k = self.get_key(key);
        self.set(&k, buf, ttl).await?;
        Ok(())
    }
    /// 从redis获取数据后使用snappy解压，并转换为对应的struct
    pub async fn get_struct_snappy<'a, T>(&self, key: &str) -> Result<T>
    where
        T: Default + DeserializeOwned,
    {
        let k = self.get_key(key);
        let value = self.get(&k).await?;

        if value.is_empty() {
            return Ok(T::default());
        }

        let buf = snappy_decode(value.as_slice())?;

        let deserializer = &mut serde_json::Deserializer::from_slice(&buf);
        T::deserialize(deserializer).context(JsonSnafu {
            category: "get_struct_snappy",
        })
    }
    /// 将struct转换为json后使用zstd压缩，
    /// 再将压缩后的数据设置至redis中，若未指定ttl，
    /// 则使用默认的有效期
    pub async fn set_struct_zstd<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_vec(&value).context(JsonSnafu {
            category: "set_struct_zstd",
        })?;
        let buf = zstd_encode(&value)?;
        let k = self.get_key(key);
        self.set(&k, buf, ttl).await?;
        Ok(())
    }
    /// 从redis获取数据后使用zstd解压，并转换为对应的struct
    pub async fn get_struct_zstd<T>(&self, key: &str) -> Result<T>
    where
        T: Default + DeserializeOwned,
    {
        let k = self.get_key(key);
        let value = self.get_bytes(&k).await?;

        if value.is_empty() {
            return Ok(T::default());
        }

        let buf = zstd_decode(value.as_slice())?;

        let deserializer = &mut serde_json::Deserializer::from_slice(&buf);
        T::deserialize(deserializer).context(JsonSnafu {
            category: "get_struct_zstd",
        })
    }
}
