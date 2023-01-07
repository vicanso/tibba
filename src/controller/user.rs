use crate::{
    controller::JSONResult,
    middleware::{
        add_session_info, get_session_info, load_session, new_session_layer, SessionInfo,
    },
};
use axum::{
    middleware::from_fn,
    routing::{get, post},
    Json, Router,
};
use axum_sessions::extractors::{ReadableSession, WritableSession};
use serde::{Deserialize, Serialize};

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

async fn me(session: ReadableSession) -> JSONResult<UserMe> {
    let info = get_session_info(session);
    Ok(Json(UserMe { name: info.account }))
    // let id: String = TRACE_ID.try_with()?;
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
