/// 缓存相关功能，支持多层级的缓存（lru+ttl)，以及
/// 封装好的各类redis操作函数
mod multi_store;
mod redis_client;

pub use multi_store::{TtlLruStore, TtlMultiStore, TtlRedisStore};
pub use redis_client::{get_default_redis_cache, must_new_redis_client, RedisCache};
