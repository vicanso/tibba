use crate::cache::must_new_redis_client;
use crate::cache::RedisSessionStore;
use crate::config::must_new_session_config;
use crate::error::{HTTPError, HTTPResult};
use crate::task_local::*;
use crate::util::{set_account_to_context, Account};
use axum::{http::Request, middleware::Next, response::Response};
use axum_sessions::extractors::{ReadableSession, WritableSession};
use axum_sessions::SessionLayer;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const SESSION_KEY: &str = "info";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionInfo {
    pub account: String,
    // 创建时间（时间戳)
    pub created_at: i64,
}

impl SessionInfo {
    // 是否应该刷新
    pub fn should_refresh(&self) -> bool {
        // 如果创建已超过一天
        if Utc::now().timestamp() - self.created_at > 24 * 3600 {
            return true;
        }
        false
    }
    pub fn logged_in(&self) -> bool {
        !self.account.is_empty()
    }
}

pub fn new_session_layer() -> SessionLayer<RedisSessionStore> {
    // TODO session加载出错时，仅状态码调整了
    // 无出错内容，且后续流程还是继续运行了
    let session_config = must_new_session_config();
    let store =
        RedisSessionStore::from_client(must_new_redis_client()).with_prefix(session_config.prefix);
    let ttl = session_config.ttl as u64;
    SessionLayer::new(store, session_config.secret.as_bytes())
        .with_secure(false)
        .with_cookie_name(session_config.cookie)
        .with_session_ttl(Some(Duration::from_secs(ttl)))
        // 仅在变化时写入
        .with_persistence_policy(axum_sessions::PersistencePolicy::ChangedOnly)
}

pub fn get_session_info(session: ReadableSession) -> SessionInfo {
    let result: Option<SessionInfo> = session.get(SESSION_KEY);
    if let Some(info) = result {
        return info;
    }
    SessionInfo::default()
}

pub fn add_session_info(mut session: WritableSession, mut info: SessionInfo) -> HTTPResult<()> {
    // 已登录的则每次设置创建时间
    if info.logged_in() {
        info.created_at = Utc::now().timestamp();
    }
    if let Err(err) = session.insert(SESSION_KEY, info) {
        return Err(HTTPError::new(&err.to_string()));
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
            // 由于在session之前的中间件无法获取account的值
            // 因此又将account设置至resp extension中
            set_account_to_context(resp.extensions_mut(), Account::new(account));
            Ok(resp)
        })
        .await
}
