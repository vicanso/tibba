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

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use base64::{Engine, engine::general_purpose::STANDARD};
use captcha::Captcha;
use captcha::filters::{Noise, Wave};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::{Error, new_error};
use tibba_state::AppState;
use tibba_util::{JsonResult, QueryParams, get_env, uuid};
use validator::Validate;
type Result<T> = std::result::Result<T, Error>;

const ERROR_CATEGORY: &str = "common_router";

/// Ping the server to check if it is running
async fn ping(State(state): State<&'static AppState>) -> Result<&'static str> {
    if !state.is_running() {
        return Err(new_error("Server is not running")
            .with_category(ERROR_CATEGORY)
            .with_status(503));
    }
    Ok("pong")
}

#[derive(Debug, Clone, Serialize)]
struct ApplicationInfo {
    uptime: String,
    env: String,
    os: String,
    arch: String,
    commit_id: String,
    hostname: String,
}

/// Get the application information
async fn get_application_info(
    State(state): State<&'static AppState>,
) -> JsonResult<ApplicationInfo> {
    let uptime = state.get_started_at().elapsed().unwrap_or_default();
    let info = os_info::get();
    let os = info.os_type().to_string();
    let arch = info.architecture().unwrap_or_default();
    let uptime_str = humantime::format_duration(uptime).to_string();
    let mut uptime_values = uptime_str.split(" ").collect::<Vec<_>>();
    if uptime_values.len() > 2 {
        uptime_values.truncate(2);
    }

    let info = ApplicationInfo {
        uptime: uptime_values.join(" "),
        env: get_env(),
        arch: arch.to_string(),
        os,
        commit_id: state.get_commit_id().to_string(),
        hostname: hostname::get()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
    };
    Ok(Json(info))
}

#[derive(Debug, Deserialize, Clone, Validate)]
pub struct CaptchaParams {
    pub preview: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct CaptchaInfo {
    id: String,
    data: String,
}

/// Generate a captcha image
async fn captcha(
    State(cache): State<&'static RedisCache>,
    QueryParams(params): QueryParams<CaptchaParams>,
) -> Result<impl IntoResponse> {
    // captcha is not supported send
    let (text, data) = {
        let mut c = Captcha::new();
        // 设置允许0会导致0的时候不展示，后续确认
        c.set_chars(&"123456789".chars().collect::<Vec<_>>())
            .add_chars(4)
            .apply_filter(Noise::new(0.4))
            .apply_filter(Wave::new(2.0, 8.0).horizontal())
            .apply_filter(Wave::new(2.0, 8.0).vertical())
            .view(120, 38);
        (c.chars_as_string(), c.as_png().unwrap_or_default())
    };

    if params.preview.unwrap_or_default() {
        let headers = [(header::CONTENT_TYPE, "image/png")];
        return Ok((headers, data).into_response());
    }

    let mut info = CaptchaInfo {
        data: STANDARD.encode(data),
        ..Default::default()
    };
    let id = uuid();
    cache
        .set(&id, &text, Some(Duration::from_secs(5 * 60)))
        .await?;
    info.id = id;

    Ok(Json(info).into_response())
}

pub struct CommonRouterParams {
    pub state: &'static AppState,
    pub cache: &'static RedisCache,
    pub secret: String,
    pub commit_id: String,
}

pub fn new_common_router(params: CommonRouterParams) -> Router {
    Router::new()
        .route("/ping", get(ping).with_state(params.state))
        .route(
            "/commons/application",
            get(get_application_info).with_state(params.state),
        )
        .route("/commons/captcha", get(captcha).with_state(params.cache))
}
