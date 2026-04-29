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

/// 错误分类标识，用于区分本路由模块产生的错误。
const ERROR_CATEGORY: &str = "common_router";

/// 存活检查接口，服务正常运行时返回 "pong"，否则返回 503。
async fn ping(State(state): State<&'static AppState>) -> Result<&'static str> {
    if !state.is_running() {
        return Err(Error::new("Server is not running")
            .with_category(ERROR_CATEGORY)
            .with_status(503));
    }
    Ok("pong")
}

/// 应用运行时信息，包含运行时长、系统环境及进程资源使用情况。
#[derive(Debug, Clone, Serialize)]
struct ApplicationInfo {
    /// 应用运行时长（人类可读格式，保留最大两个单位，如 "2h 15m"）
    uptime: String,
    /// 当前运行环境（development / production 等）
    env: String,
    /// 操作系统类型
    os: String,
    /// CPU 架构
    arch: String,
    /// 当前部署的 Git commit ID
    commit_id: String,
    /// 主机名
    hostname: String,
    /// 进程内存占用（MB）
    memory_usage_mb: u32,
    /// 进程 CPU 使用率（百分比整数）
    cpu_usage: u32,
    /// 打开的文件描述符数量
    open_files: u32,
    /// 进程启动以来写入磁盘总量（MB）
    total_written_mb: u32,
    /// 进程启动以来从磁盘读取总量（MB）
    total_read_mb: u32,
    /// 服务是否处于运行中状态
    running: bool,
}

/// 将 Duration 格式化为人类可读字符串，只保留最大两个时间单位。
/// 例如 "2h 15m 30s" → "2h 15m"。
fn format_uptime_approx(duration: Duration) -> String {
    humantime::format_duration(duration)
        .to_string()
        .split(' ')
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 返回应用运行时信息，包含运行时长、OS/架构、CPU/内存占用及磁盘读写量。
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

/// 验证码接口的查询参数。
#[derive(Debug, Deserialize, Clone, Validate)]
pub struct CaptchaParams {
    /// 为 `true` 时直接返回 PNG 图片（开发预览用），不写入 Redis。
    pub preview: Option<bool>,
    /// 主题，`"dark"` 时对图片颜色取反以适配深色背景。
    pub theme: Option<String>,
}

/// 验证码接口的 JSON 响应体。
#[derive(Debug, Clone, Serialize, Default)]
struct CaptchaInfo {
    /// 验证码唯一标识，用于后续校验时从 Redis 取出正确答案。
    id: String,
    /// Base64 编码的 PNG 图片数据。
    data: String,
}

/// 生成图形验证码。
///
/// - `preview=true`：直接返回原始 PNG（用于开发调试），不写入 Redis。
/// - 默认：生成 4 位纯数字验证码（排除 '0' 以避免与字母 'O' 混淆），
///   将答案以 UUID 为键存入 Redis（TTL 5 分钟），返回 `{ id, data }` JSON。
/// - `theme=dark`：生成后对图片颜色取反，适配深色主题。
async fn captcha(
    State(cache): State<&'static RedisCache>,
    QueryParams(params): QueryParams<CaptchaParams>,
) -> Result<impl IntoResponse> {
    let is_dark = params.theme.unwrap_or_default() == "dark";
    // 字符集排除 '0'，避免与字母 'O' 视觉混淆
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
            // 深色主题：对 PNG 图片进行颜色取反处理
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

    // 预览模式：直接返回 PNG 图片
    if params.preview.unwrap_or_default() {
        let headers = [(header::CONTENT_TYPE, "image/png")];
        return Ok((headers, data).into_response());
    }

    // 将验证码答案存入 Redis，TTL 5 分钟
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

/// 构建公共路由所需的参数。
pub struct CommonRouterParams {
    /// 全局应用状态
    pub state: &'static AppState,
    /// Redis 缓存实例，为 `None` 时不注册验证码路由。
    pub cache: Option<&'static RedisCache>,
}

/// 创建公共路由，包含以下端点：
/// - `GET /ping` — 存活检查
/// - `GET /commons/application` — 应用运行时信息
/// - `GET /commons/captcha` — 图形验证码生成（仅当 `cache` 不为 `None` 时注册）
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
