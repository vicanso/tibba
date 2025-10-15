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
use std::io::Cursor;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::Error;
use tibba_performance::get_process_system_info;
use tibba_state::AppState;
use tibba_util::{JsonResult, QueryParams, get_env, uuid};
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

const ERROR_CATEGORY: &str = "common_router";

/// Ping the server to check if it is running
async fn ping(State(state): State<&'static AppState>) -> Result<&'static str> {
    if !state.is_running() {
        return Err(Error::new("Server is not running")
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
    memory_usage_mb: u32,
    cpu_usage: u32,
    open_files: u32,
    total_written_mb: u32,
    total_read_mb: u32,
    running: bool,
}

fn format_uptime_approx(duration: Duration) -> String {
    humantime::format_duration(duration)
        .to_string()
        .split(' ')
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Get the application information
async fn get_application_info(
    State(state): State<&'static AppState>,
) -> JsonResult<ApplicationInfo> {
    let uptime = state.get_started_at().elapsed().unwrap_or_default();
    let info = os_info::get();
    let os = info.os_type().to_string();
    let arch = info.architecture().unwrap_or_default();
    let performance = get_process_system_info(std::process::id() as usize);
    let mb = 1024 * 1024;

    let info = ApplicationInfo {
        uptime: format_uptime_approx(uptime),
        env: get_env().to_string(),
        arch: arch.to_string(),
        os,
        commit_id: state.get_commit_id().to_string(),
        hostname: hostname::get()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        cpu_usage: performance.cpu_usage as u32,
        memory_usage_mb: (performance.memory_usage / mb) as u32,
        open_files: performance.open_files.unwrap_or_default() as u32,
        total_written_mb: (performance.total_written_bytes / mb) as u32,
        total_read_mb: (performance.total_read_bytes / mb) as u32,
        running: state.is_running(),
    };
    Ok(Json(info))
}

#[derive(Debug, Deserialize, Clone, Validate)]
pub struct CaptchaParams {
    pub preview: Option<bool>,
    pub theme: Option<String>,
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
    let is_dark = params.theme.unwrap_or_default() == "dark";
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
        if is_dark {
            c.set_color([60, 60, 60]);
        }
        let buffer = c.as_png().unwrap_or_default();
        if is_dark {
            let mut img = image::load_from_memory(&buffer)
                .map_err(|e| Error::new(e.to_string()).with_exception(true))?;
            img.invert();
            let mut buffer: Vec<u8> = Vec::new();
            img.write_to(&mut Cursor::new(&mut buffer), image::ImageFormat::Png)
                .map_err(|e| Error::new(e.to_string()).with_exception(true))?;
            (c.chars_as_string(), buffer)
        } else {
            (c.chars_as_string(), buffer)
        }
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
    pub cache: Option<&'static RedisCache>,
    pub commit_id: String,
}

pub fn new_common_router(params: CommonRouterParams) -> Router {
    let r = Router::new()
        .route("/ping", get(ping).with_state(params.state))
        .route(
            "/commons/application",
            get(get_application_info).with_state(params.state),
        );
    let Some(cache) = params.cache else {
        return r;
    };

    r.route("/commons/captcha", get(captcha).with_state(cache))
}
