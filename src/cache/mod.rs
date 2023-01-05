mod multi_store;
mod redis_client;

pub use redis_client::{must_new_redis_client, RedisCache};
