mod redis_client;
/// 缓存相关功能，支持种缓存（lru+ttl)，以及
/// 封装好的各类redis操作函数
mod ttl_lru_store;

pub use redis_client::{get_default_redis_cache, must_new_redis_client, RedisCache};
pub use ttl_lru_store::TtlLruStore;
