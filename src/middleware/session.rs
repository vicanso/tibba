use async_redis_session::RedisSessionStore;
use axum_sessions::{
    extractors::{ReadableSession, WritableSession},
    SessionLayer,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{cache::must_new_redis_client, error::HTTPError, util::Context};

const SESSION_KEY: &str = "info";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionInfo {
    pub account: String,
}

pub fn new_session_layer() -> SessionLayer<RedisSessionStore> {
    // TODO redis、secret 从配置中获取
    let store = RedisSessionStore::from_client(must_new_redis_client()).with_prefix("ss:");
    let secret = "random string random string random string random string random string".as_bytes();
    SessionLayer::new(store, secret)
        .with_secure(false)
        .with_cookie_name("tibba")
        .with_session_ttl(Some(Duration::from_secs(7 * 24 * 3600)))
}

pub fn get_session_info(_ctx: Context, session: ReadableSession) -> SessionInfo {
    let result: Option<SessionInfo> = session.get(SESSION_KEY);
    if let Some(info) = result {
        return info;
    }
    SessionInfo::default()
}

pub fn add_session_info(
    _ctx: Context,
    mut session: WritableSession,
    info: SessionInfo,
) -> Result<(), HTTPError> {
    if let Err(err) = session.insert(SESSION_KEY, info) {
        return Err(HTTPError::new(err.to_string().as_str()));
    }
    Ok(())
}
