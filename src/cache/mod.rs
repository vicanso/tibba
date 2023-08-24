mod redis_client;
mod redis_session_store;
/// 缓存相关功能，支持种缓存（lru+ttl)，以及
/// 封装好的各类redis操作函数
mod ttl_lru_store;
mod two_level_store;

pub use redis_client::{get_default_redis_cache, must_get_redis_pool, redis_ping, RedisCache};
pub use redis_session_store::RedisSessionStore;
pub use ttl_lru_store::TtlLruStore;
pub use two_level_store::TwoLevelStore;

pub(self) use redis_client::get_redis_conn;
pub(self) use redis_client::Result as RedisResult;
pub(self) use ttl_lru_store::Expired;
