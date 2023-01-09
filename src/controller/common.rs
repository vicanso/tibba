use axum::{routing::get, Json, Router};
use chrono::{DateTime, Duration, Utc};
use once_cell::sync::OnceCell;
use serde::Serialize;

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
    uptime: String,
    env: String,
    os: String,
}

static STARTED_AT: OnceCell<DateTime<Utc>> = OnceCell::new();

pub fn new_router() -> Router {
    STARTED_AT.get_or_init(Utc::now);
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

async fn get_application_info() -> HTTPResult<Json<ApplicationInfo>> {
    let d = {
        if let Some(value) = STARTED_AT.get() {
            Utc::now().signed_duration_since(*value)
        } else {
            Duration::nanoseconds(0)
        }
    };
    let os = os_info::get().os_type().to_string();

    let info = ApplicationInfo {
        builded_at: asset::get_build_date(),
        uptime: duration_to_string(d),
        env: get_env(),
        os,
    };
    Ok(Json(info))
}
