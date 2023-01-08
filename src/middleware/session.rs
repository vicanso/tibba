use async_redis_session::RedisSessionStore;
use axum::{http::Request, middleware::Next, response::Response};
use axum_sessions::{
    extractors::{ReadableSession, WritableSession},
    SessionLayer,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{
    cache::must_new_redis_client,
    config::must_new_session_config,
    error::{HTTPError, HTTPResult},
    util::{set_account_to_context, Account, ACCOUNT},
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

pub fn get_session_info(session: ReadableSession) -> SessionInfo {
    let result: Option<SessionInfo> = session.get(SESSION_KEY);
    if let Some(info) = result {
        return info;
    }
    SessionInfo::default()
}

pub fn add_session_info(mut session: WritableSession, info: SessionInfo) -> HTTPResult<()> {
    if let Err(err) = session.insert(SESSION_KEY, info) {
        return Err(HTTPError::new(err.to_string().as_str()));
    }
    Ok(())
}

pub async fn load_session<B>(
    session: ReadableSession,
    req: Request<B>,
    next: Next<B>,
) -> HTTPResult<Response> {
    let info = get_session_info(session);

    let account = info.account.clone();
    ACCOUNT
        .scope(info.account, async {
            let mut resp = next.run(req).await;
            set_account_to_context(resp.extensions_mut(), Account::new(account));
            Ok(resp)
        })
        .await
}
