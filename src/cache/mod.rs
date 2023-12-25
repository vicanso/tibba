mod redis_client;
/// 缓存相关功能，支持种缓存（lru+ttl)，以及
/// 封装好的各类redis操作函数
mod ttl_lru_store;
mod two_level_store;

pub use redis_client::{get_default_redis_cache, redis_ping, RedisCache};
pub use ttl_lru_store::TtlLruStore;
pub use two_level_store::TwoLevelStore;

pub(self) use redis_client::Result as RedisResult;
pub(self) use ttl_lru_store::Expired;

#[cfg(not(feature = "redis_cluster"))]
mod redis_normal_pool;
#[cfg(not(feature = "redis_cluster"))]
pub(self) use redis_normal_pool::must_get_redis_pool;

#[cfg(feature = "redis_cluster")]
mod redis_cluster_pool;
#[cfg(feature = "redis_cluster")]
pub(self) use redis_cluster_pool::must_get_redis_pool;
