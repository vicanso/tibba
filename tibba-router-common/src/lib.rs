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
use tibba_util::{CacheJsonResult, Query, get_env, uuid};
use validator::Validate;
type Result<T> = std::result::Result<T, Error>;

const ERROR_CATEGORY: &str = "common_router";

async fn ping(State(state): State<&'static AppState>) -> Result<&'static str> {
    if !state.is_running() {
        return Err(new_error("Server is not running")
            .with_category(ERROR_CATEGORY)
            .with_status(503)
            .into());
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

#[derive(Debug, Deserialize, Clone, Validate)]
pub struct CaptchaParams {
    pub preview: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct CaptchaInfo {
    hash: String,
    data: String,
}

async fn captcha(
    State(cache): State<&'static RedisCache>,
    Query(params): Query<CaptchaParams>,
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
    let hash = uuid();
    cache
        .set(&hash, &text, Some(Duration::from_secs(5 * 60)))
        .await?;
    info.hash = hash;

    Ok(Json(info).into_response())
}

pub struct CommonRouterParams {
    pub state: &'static AppState,
    pub cache: &'static RedisCache,
    pub secret: String,
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
