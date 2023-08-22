use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use axum_sessions::extractors::{ReadableSession, WritableSession};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::cache::get_default_redis_cache;
use crate::controller::JsonResult;
use crate::error::HttpResult;
use crate::middleware::{
    add_session_info, get_session_info, load_session, new_session_layer, wait1s, SessionInfo,
};
use crate::util::{generate_device_id_cookie, get_device_id_from_cookie};

#[derive(Debug, Clone, Serialize, Default)]
struct UserMeResp {
    name: String,
    should_refresh: bool,
    time: String,
}

pub fn new_router() -> Router {
    let login_router = Router::new()
        .route("/v1/login", post(login))
        // 登录设置为最少等待x秒，避免快速尝试
        .layer(from_fn(wait1s));
    let r = Router::new()
        .route("/v1/me", get(me))
        .route("/v1/login-times", get(get_login_times))
        .merge(login_router)
        .layer(from_fn(load_session))
        .layer(new_session_layer());

    Router::new().nest("/users", r)
}

async fn me(
    session: ReadableSession,
    mut jar: CookieJar,
) -> HttpResult<(CookieJar, Json<UserMeResp>)> {
    let info = get_session_info(session);
    let mut should_refresh = false;
    // 如果已登录
    if info.is_login() && info.should_refresh() {
        should_refresh = true
    }
    let me = UserMeResp {
        name: info.account,
        should_refresh,
        time: Local::now().to_rfc3339(),
    };
    // 如果未设置device，则设置
    if get_device_id_from_cookie(&jar).is_empty() {
        jar = jar.add(generate_device_id_cookie());
    }
    Ok((jar, Json(me)))
}

#[derive(Deserialize)]
struct LoginParams {
    account: String,
}

async fn login(
    session: WritableSession,
    Json(params): Json<LoginParams>,
) -> JsonResult<UserMeResp> {
    add_session_info(
        session,
        SessionInfo {
            account: params.account.clone(),
            ..Default::default()
        },
    )?;
    Ok(Json(UserMeResp {
        name: params.account,
        time: Local::now().to_rfc3339(),
        ..Default::default()
    }))
}

#[derive(Debug, Clone, Serialize, Default)]
struct LoginTimesResp {
    pub count: i64,
}
async fn get_login_times(jar: CookieJar) -> JsonResult<LoginTimesResp> {
    let device_id = get_device_id_from_cookie(&jar);
    let cache = get_default_redis_cache();
    // 如果未设置device，则设置
    let mut count: i64 = 0;
    if !device_id.is_empty() {
        count = cache
            .incr(&device_id, 1, Some(Duration::from_secs(60 * 60)))
            .await?;
    }

    Ok(Json(LoginTimesResp { count }))
}
