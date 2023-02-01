use axum::{routing::get, Json, Router};
use chrono::Utc;
use serde::Serialize;

use super::JSONResult;
use crate::{
    asset,
    config::get_env,
    error::{HTTPError, HTTPResult},
    state::get_app_state,
    util::duration_to_string,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationInfo {
    builded_at: String,
    commit: String,
    uptime: String,
    env: String,
    os: String,
}

pub fn new_router() -> Router {
    let r = Router::new().route("/application", get(get_application_info));

    Router::new().route("/ping", get(ping)).nest("/commons", r)
}

async fn ping() -> HTTPResult<&'static str> {
    let state = get_app_state();
    if !state.is_running() {
        return Err(HTTPError::new("server is not running"));
    }
    Ok("pong")
}

async fn get_application_info() -> JSONResult<ApplicationInfo> {
    let app_state = get_app_state();
    let d = Utc::now().signed_duration_since(app_state.get_started_at());
    let os = os_info::get().os_type().to_string();

    let info = ApplicationInfo {
        builded_at: asset::get_build_date(),
        commit: asset::get_commit(),
        uptime: duration_to_string(d),
        env: get_env(),
        os,
    };
    Ok(Json(info))
}
