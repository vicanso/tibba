use redis::Client;

use crate::config::must_new_redis_config;

pub fn must_new_redis_client() -> Client {
    let config = must_new_redis_config();
    Client::open(config.uri).unwrap()
}
