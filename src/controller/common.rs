use super::{CacheJsonResult, JsonResult, Query};
use crate::config::get_env;
use crate::error::{HttpError, HttpResult};
use crate::state::get_app_state;
use crate::{asset, cache, util};
use axum::{routing::get, Router};
use captcha::filters::{Noise, Wave};
use captcha::Captcha;
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
    let r = Router::new()
        .route("/application", get(get_application_info))
        .route("/captcha", get(captcha));

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
    let uptime = util::get_duration_string(&app_state.get_started_at());
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
        uptime,
        env: get_env(),
        arch: arch.to_string(),
        os,
        version: VERSION.to_string(),
    };
    Ok((Duration::from_secs(60), info).into())
}

#[derive(Debug, Deserialize, Clone)]
pub struct CaptchaParams {
    pub level: Option<i8>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct CaptchaInfo {
    ts: i64,
    hash: String,
    data: String,
}

async fn captcha(Query(params): Query<CaptchaParams>) -> JsonResult<CaptchaInfo> {
    let level = params.level.unwrap_or_default();
    // 未实现send，因此需要将其生命周期减短
    let (text, data) = {
        let mut c = Captcha::new();
        c.add_chars(4)
            .apply_filter(Noise::new(0.2))
            .apply_filter(Wave::new(2.0, 8.0).horizontal())
            .apply_filter(Wave::new(2.0, 8.0).vertical())
            .view(120, 40);
        (c.chars_as_string(), c.as_base64().unwrap_or_default())
    };
    let mut info = CaptchaInfo {
        data,
        ..Default::default()
    };
    if level > 0 {
        let hash = util::uuid();
        cache::get_default_redis_cache()
            .set_string(&hash, &text, Some(Duration::from_secs(5 * 60)))
            .await?;
        info.hash = hash;
    } else {
        let (ts, hash) = util::timestamp_hash(&text);
        info.ts = ts;
        info.hash = hash;
    }

    Ok(info.into())
}
