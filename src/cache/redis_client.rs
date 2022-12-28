use redis::Client;

pub fn must_new_redis_client() -> Client {
    // TODO 配置从config中获取
    Client::open("redis://127.0.0.1/").unwrap()
}
