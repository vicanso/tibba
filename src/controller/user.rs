use axum::{
    middleware::from_fn,
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::CookieJar;
use axum_sessions::extractors::{ReadableSession, WritableSession};
use serde::{Deserialize, Serialize};

use crate::{
    controller::JSONResult,
    error::HTTPResult,
    middleware::{
        add_session_info, get_session_info, load_session, new_session_layer, SessionInfo,
    },
    util::{generate_device_id_cookie, get_device_id_from_cookie},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserMe {
    name: String,
}

pub fn new_router() -> Router {
    let r = Router::new()
        .route("/v1/me", get(me))
        .route("/v1/login", post(login))
        .layer(from_fn(load_session))
        .layer(new_session_layer());

    Router::new().nest("/users", r)
}

async fn me(session: ReadableSession, mut jar: CookieJar) -> HTTPResult<(CookieJar, Json<UserMe>)> {
    let info = get_session_info(session);
    let me = UserMe { name: info.account };
    // 如果未设置device，则设置
    if get_device_id_from_cookie(&jar).is_empty() {
        jar = jar.add(generate_device_id_cookie());
    }
    Ok((jar, Json(me)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginParams {
    account: String,
}

async fn login(session: WritableSession, Json(params): Json<LoginParams>) -> JSONResult<UserMe> {
    add_session_info(
        session,
        SessionInfo {
            account: params.account.clone(),
        },
    )?;
    Ok(Json(UserMe {
        name: params.account,
    }))
}
