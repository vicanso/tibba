use axum::{
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMe {
    name: String,
}

pub fn new_router() -> Router {
    let r = Router::new()
        .route("/v1/me", get(me))
        .route("/v1/login", post(login));

    Router::new().nest("/users", r)
}

async fn me() -> impl IntoResponse {
    Json(UserMe {
        name: "tree.xie".to_string(),
    })
}

async fn login() -> impl IntoResponse {
    Json(UserMe {
        name: "tree.xie".to_string(),
    })
}
