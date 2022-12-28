use crate::{
    controller::JSONResult,
    middleware::{add_session_info, get_session_info, new_session_layer, SessionInfo},
    util::Context,
};
use axum::{
    routing::{get, post},
    Extension, Json, Router,
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
        .layer(new_session_layer());

    Router::new().nest("/users", r)
}

async fn me(session: ReadableSession, Extension(ctx): Extension<Context>) -> JSONResult<UserMe> {
    let info = get_session_info(ctx, session);
    Ok(Json(UserMe { name: info.account }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginParams {
    account: String,
}

async fn login(
    session: WritableSession,
    Extension(ctx): Extension<Context>,
    Json(params): Json<LoginParams>,
) -> JSONResult<UserMe> {
    add_session_info(
        ctx,
        session,
        SessionInfo {
            account: params.account.clone(),
        },
    )?;
    Ok(Json(UserMe {
        name: params.account,
    }))
}
