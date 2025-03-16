// Copyright 2025 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde::Serialize;
use std::time::Duration;
use tibba_error::{Error, new_error_with_category};
use tibba_state::AppState;
use tibba_util::{CacheJsonResult, get_env};

type Result<T> = std::result::Result<T, Error>;

const ERROR_CATEGORY: &str = "common_router";

async fn ping(State(state): State<&'static AppState>) -> Result<&'static str> {
    if !state.is_running() {
        return Err(new_error_with_category(
            "Server is not running".to_string(),
            ERROR_CATEGORY.to_string(),
        ));
    }
    Ok("pong")
}

#[derive(Debug, Clone, Serialize)]
struct ApplicationInfo {
    uptime: String,
    env: String,
    os: String,
    arch: String,
}

async fn get_application_info(
    State(state): State<&'static AppState>,
) -> CacheJsonResult<ApplicationInfo> {
    let uptime = state.get_started_at().elapsed().unwrap_or_default();
    let os = os_info::get().os_type().to_string();
    let mut arch = "x86";
    // simple detection
    if cfg!(any(target_arch = "arm", target_arch = "aarch64")) {
        arch = "arm64"
    }

    let info = ApplicationInfo {
        uptime: humantime::format_duration(uptime).to_string(),
        env: get_env(),
        arch: arch.to_string(),
        os,
    };
    Ok((Duration::from_secs(60), info).into())
}

pub struct CommonRouterParams {
    pub state: &'static AppState,
}

pub fn new_common_router(params: CommonRouterParams) -> Router {
    Router::new()
        .route("/ping", get(ping))
        .route("/commons/info", get(get_application_info))
        .with_state(params.state)
}
