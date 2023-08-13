use chrono::Local;
use std::time::Duration;

mod redis_client;
mod redis_session_store;
/// 缓存相关功能，支持种缓存（lru+ttl)，以及
/// 封装好的各类redis操作函数
mod ttl_lru_store;

pub use redis_client::{get_default_redis_cache, get_redis_conn, redis_ping, RedisCache};
pub use redis_session_store::RedisSessionStore;
pub use ttl_lru_store::TtlLruStore;

// 根据当前时间以及unit计算有效期，让有效期尽可能落在间隔点
pub fn get_ttl_by_unit(unit: Duration) -> Duration {
    let now = Local::now();
    // 计算剩下的时间
    let seconds = unit.as_secs() - (now.timestamp() as u64 % unit.as_secs());
    // 如果少于1/10
    if seconds < unit.as_secs() / 10 {
        return Duration::from_secs(unit.as_secs() + seconds);
    }
    Duration::from_secs(seconds)
}
