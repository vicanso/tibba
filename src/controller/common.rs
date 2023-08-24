use super::CacheJsonResult;
use crate::asset;
use crate::config::get_env;
use crate::error::{HttpError, HttpResult};
use crate::state::get_app_state;
use crate::util::duration_to_string;
use axum::{routing::get, Router};
use chrono::Utc;
use serde::Serialize;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize)]
struct ApplicationInfo {
    builded_at: String,
    commit: String,
    uptime: String,
    env: String,
    os: String,
    arch: String,
    version: String,
}

pub fn new_router() -> Router {
    let r = Router::new().route("/application", get(get_application_info));

    Router::new().route("/ping", get(ping)).nest("/commons", r)
}

async fn ping() -> HttpResult<&'static str> {
    let state = get_app_state();
    if !state.is_running() {
        return Err(HttpError::new("Server is not running"));
    }
    Ok("pong")
}

async fn get_application_info() -> CacheJsonResult<ApplicationInfo> {
    let app_state = get_app_state();
    let d = Utc::now().signed_duration_since(app_state.get_started_at());
    let os = os_info::get().os_type().to_string();
    let mut arch = "x86";
    // 运行环境较为单一，此字段也只用于展示
    // 因此简单判断
    if cfg!(any(target_arch = "arm", target_arch = "aarch64")) {
        arch = "arm64"
    }

    let info = ApplicationInfo {
        builded_at: asset::get_build_date(),
        commit: asset::get_commit(),
        uptime: duration_to_string(d),
        env: get_env(),
        arch: arch.to_string(),
        os,
        version: VERSION.to_string(),
    };
    Ok((60, info).into())
}
