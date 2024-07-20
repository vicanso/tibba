use crate::error::HttpError;
use crate::util::CompressError;
use snafu::Snafu;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("category: {category}, error: {message}"))]
    Common { category: String, message: String },
    #[snafu(display("{source}"))]
    SingleBuild { source: deadpool_redis::BuildError },
    #[snafu(display("{source}"))]
    ClusterBuild {
        source: deadpool_redis::cluster::CreatePoolError,
    },
    #[snafu(display("category: {category}, error: {source}"))]
    Redis {
        category: String,
        source: deadpool_redis::redis::RedisError,
    },
    #[snafu(display("{source}"))]
    Compress { source: CompressError },
}
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<Error> for HttpError {
    fn from(err: Error) -> Self {
        match err {
            Error::Common { message, category } => {
                HttpError::new_with_category(&message, &format!("cache_{category}"))
            }
            Error::SingleBuild { source } => {
                HttpError::new_with_category(&source.to_string(), "cache_single_build")
            }
            Error::ClusterBuild { source } => {
                HttpError::new_with_category(&source.to_string(), "cache_cluster_build")
            }
            Error::Redis { category, source } => {
                let mut msg = source.to_string();
                if msg.contains("(response was nil)") {
                    msg = "数据已过期".to_string();
                }
                HttpError::new_with_category(&msg, &category)
            }
            Error::Compress { source } => source.into(),
        }
    }
}

mod redis_client;
mod redis_pool;
/// 缓存相关功能，支持种缓存（lru+ttl)，以及
/// 封装好的各类redis操作函数
mod ttl_lru_store;
mod two_level_store;

pub use redis_client::{get_default_redis_cache, redis_ping, RedisCache};
pub use ttl_lru_store::TtlLruStore;
pub use two_level_store::TwoLevelStore;

pub(self) use ttl_lru_store::Expired;
