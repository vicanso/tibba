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
use tibba_performance::current_process_system_info;
use tibba_state::AppState;
use tibba_util::{JsonResult, QueryParams, get_env, uuid};
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

const ERROR_CATEGORY: &str = "common_router";

/// Returns "pong" when the server is running, 503 otherwise.
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

/// Formats a duration as a human-readable string, keeping only the two largest units.
/// Example: "2h 15m" instead of "2h 15m 30s 500ms".
fn format_uptime_approx(duration: Duration) -> String {
    humantime::format_duration(duration)
        .to_string()
        .split(' ')
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns application runtime information including uptime, OS, CPU/memory usage, and disk I/O.
async fn get_application_info(
    State(state): State<&'static AppState>,
) -> JsonResult<ApplicationInfo> {
    let uptime = state.get_started_at().elapsed().unwrap_or_default();
    let os_info = os_info::get();
    let performance = current_process_system_info();
    let mb = 1024 * 1024;

    Ok(Json(ApplicationInfo {
        uptime: format_uptime_approx(uptime),
        env: get_env().to_string(),
        arch: os_info.architecture().unwrap_or_default().to_string(),
        os: os_info.os_type().to_string(),
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
    }))
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

/// Generates a CAPTCHA image.
///
/// When `preview=true` the raw PNG is returned directly (for development).
/// Otherwise the PNG is base64-encoded, stored in Redis with a 5-minute TTL,
/// and the generated ID + encoded image are returned as JSON.
///
/// When `theme=dark` the image colours are inverted after generation.
async fn captcha(
    State(cache): State<&'static RedisCache>,
    QueryParams(params): QueryParams<CaptchaParams>,
) -> Result<impl IntoResponse> {
    let is_dark = params.theme.unwrap_or_default() == "dark";
    // Exclude '0' to avoid confusion with the letter 'O'
    let (text, data) = {
        let mut c = Captcha::new();
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

    let id = uuid();
    cache
        .set(&id, &text, Some(Duration::from_secs(5 * 60)))
        .await?;
    Ok(Json(CaptchaInfo {
        id,
        data: STANDARD.encode(data),
    })
    .into_response())
}

/// Parameters for constructing the common router.
pub struct CommonRouterParams {
    pub state: &'static AppState,
    pub cache: Option<&'static RedisCache>,
}

/// Creates a [`Router`] with the following routes:
/// - `GET /ping` — liveness check
/// - `GET /commons/application` — application runtime info
/// - `GET /commons/captcha` — CAPTCHA generation (only when `cache` is provided)
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
