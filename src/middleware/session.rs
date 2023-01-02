use async_redis_session::RedisSessionStore;
use axum_sessions::{
    extractors::{ReadableSession, WritableSession},
    SessionLayer,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{
    cache::must_new_redis_client, config::must_new_session_config, error::HTTPError, util::Context,
};

const SESSION_KEY: &str = "info";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionInfo {
    pub account: String,
}

pub fn new_session_layer() -> SessionLayer<RedisSessionStore> {
    let session_config = must_new_session_config();
    let store =
        RedisSessionStore::from_client(must_new_redis_client()).with_prefix(session_config.prefix);
    let ttl = session_config.ttl as u64;
    SessionLayer::new(store, session_config.secret.as_bytes())
        .with_secure(false)
        .with_cookie_name(session_config.cookie)
        .with_session_ttl(Some(Duration::from_secs(ttl)))
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
