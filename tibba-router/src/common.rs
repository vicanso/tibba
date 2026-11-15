// Copyright 2026 Tree xie.
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
use axum::routing::get;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tibba_auth::AdminSession;
use tibba_error::Error as BaseError;
use tibba_runtime::AppState;
#[cfg(feature = "process-info")]
use tibba_runtime::current_process_system_info_async;
use tibba_util::{JsonResult, get_env};
use tokio::time::timeout;
use tracing::warn;
use utoipa::{OpenApi, ToSchema};

type Result<T, E = BaseError> = std::result::Result<T, E>;

/// 该模块所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:router:common=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:router:common";

/// 错误分类标识，用于区分本路由模块产生的错误。
const ERROR_CATEGORY: &str = "common_router";

/// 进程生命周期内不变的系统元数据 `(os, arch, hostname)`，惰性缓存避免每次请求重复 syscall。
static STATIC_SYS_INFO: LazyLock<(String, String, String)> = LazyLock::new(|| {
    let info = os_info::get();
    let os = info.os_type().to_string();
    let arch = info.architecture().unwrap_or_default().to_string();
    let hostname = hostname::get()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    (os, arch, hostname)
});

/// readiness 探针超时；超过即视为未就绪，K8s 会摘流量但**不**重启 Pod。
const READINESS_TIMEOUT: Duration = Duration::from_secs(2);

/// readiness 回调的 Future 类型。成功表示依赖可用，错误会被记录到响应。
pub type ReadinessFuture = Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>;
/// readiness 回调签名：无参，返回 [`ReadinessFuture`]。
/// 由调用方在 [`CommonRouterParams::readiness`] 注入，通常包含 storage / db
/// 等外部依赖的轻量探活；未注入时 `/readyz` 仅检查 `state.is_running()`。
pub type ReadinessCheck = Arc<dyn Fn() -> ReadinessFuture + Send + Sync>;

/// Liveness 探针：仅检查应用自身是否处于运行态，**不**做任何外部 I/O。
/// K8s 失败时会重启 Pod，因此这里必须保持轻量、绝不依赖下游。
#[utoipa::path(
    get,
    path = "/healthz",
    tag = "common",
    responses(
        (status = 200, description = "服务存活", body = String),
        (status = 503, description = "服务未处于运行态")
    )
)]
async fn healthz(State(state): State<&'static AppState>) -> Result<&'static str> {
    if !state.is_running() {
        return Err(BaseError::new("server is not running")
            .with_category(ERROR_CATEGORY)
            .with_status(503));
    }
    Ok("ok")
}

/// Readiness 探针：先确认应用运行态，再调用注入的 readiness 回调（如 stat
/// storage / ping db）；超时或失败均返回 503，由 K8s 摘流量但不重启 Pod。
#[utoipa::path(
    get,
    path = "/readyz",
    tag = "common",
    responses(
        (status = 200, description = "依赖就绪", body = String),
        (status = 503, description = "未运行 / 依赖检查失败 / 超时")
    )
)]
async fn readyz(
    State((state, check)): State<(&'static AppState, Option<ReadinessCheck>)>,
) -> Result<&'static str> {
    if !state.is_running() {
        return Err(BaseError::new("server is not running")
            .with_category(ERROR_CATEGORY)
            .with_status(503));
    }
    let Some(check) = check else {
        // 没有注入依赖检查时，readiness 与 liveness 等价
        return Ok("ready");
    };
    match timeout(READINESS_TIMEOUT, check()).await {
        Ok(Ok(())) => Ok("ready"),
        Ok(Err(err)) => {
            warn!(target: LOG_TARGET, error = %err, "readiness check failed");
            Err(BaseError::new(format!("readiness check failed: {err}"))
                .with_category(ERROR_CATEGORY)
                .with_status(503))
        }
        Err(_) => {
            warn!(
                target: LOG_TARGET,
                timeout_secs = READINESS_TIMEOUT.as_secs(),
                "readiness check timed out"
            );
            Err(BaseError::new(format!(
                "readiness check timed out after {}s",
                READINESS_TIMEOUT.as_secs()
            ))
            .with_category(ERROR_CATEGORY)
            .with_status(503))
        }
    }
}

/// 应用运行时信息，包含运行时长、系统环境及进程资源使用情况。
#[derive(Debug, Clone, Serialize, ToSchema)]
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

/// 返回应用运行时信息，包含运行时长、OS/架构，启用 `process-info` 时采集 CPU/内存/磁盘 I/O。
#[utoipa::path(
    get,
    path = "/commons/application",
    tag = "common",
    responses(
        (status = 200, description = "应用运行时信息", body = ApplicationInfo)
    )
)]
async fn get_application_info(
    // 要求 Admin：该端点暴露 hostname / commit / OS / 内存 CPU 等内部信息，不对匿名开放
    _admin: AdminSession,
    State(state): State<&'static AppState>,
) -> JsonResult<ApplicationInfo> {
    let uptime = state.get_started_at().elapsed().unwrap_or_default();
    let (os, arch, hostname) = &*STATIC_SYS_INFO;

    #[cfg(feature = "process-info")]
    let (cpu_usage, memory_usage_mb, open_files, total_written_mb, total_read_mb) = {
        // 采样是同步系统调用，丢到阻塞线程池，避免堵住 tokio worker
        let performance = current_process_system_info_async().await;
        let mb = 1024 * 1024;
        (
            performance.cpu_usage as u32,
            (performance.memory_usage / mb) as u32,
            performance.open_files.unwrap_or_default() as u32,
            (performance.total_written_bytes / mb) as u32,
            (performance.total_read_bytes / mb) as u32,
        )
    };
    #[cfg(not(feature = "process-info"))]
    let (cpu_usage, memory_usage_mb, open_files, total_written_mb, total_read_mb) = (0, 0, 0, 0, 0);

    Ok(Json(ApplicationInfo {
        uptime: format_uptime_approx(uptime),
        env: get_env().to_string(),
        arch: arch.clone(),
        os: os.clone(),
        commit_id: state.get_commit_id().to_string(),
        hostname: hostname.clone(),
        cpu_usage,
        memory_usage_mb,
        open_files,
        total_written_mb,
        total_read_mb,
        running: state.is_running(),
    }))
}

/// 构建公共路由所需的参数。
pub struct CommonRouterParams {
    /// 全局应用状态
    pub state: &'static AppState,
    /// readiness 回调；`None` 时 `/readyz` 仅检查应用运行态。
    /// 用于在 readiness 探针中验证外部依赖（storage / db / 第三方服务）可达。
    pub readiness: Option<ReadinessCheck>,
}

/// 创建公共路由，包含以下端点：
/// - `GET /healthz` — Liveness 探针（仅检查 `AppState`，失败由 K8s 重启 Pod）
/// - `GET /readyz` — Readiness 探针（含依赖检查，2s 超时，失败由 K8s 摘流量）
/// - `GET /commons/application` — 应用运行时信息
pub fn new_common_router(params: CommonRouterParams) -> Router {
    Router::new()
        .route("/healthz", get(healthz).with_state(params.state))
        .route(
            "/readyz",
            get(readyz).with_state((params.state, params.readiness)),
        )
        .route(
            "/commons/application",
            get(get_application_info).with_state(params.state),
        )
}

/// 本路由模块的 OpenAPI 文档片段。
#[derive(OpenApi)]
#[openapi(
    paths(healthz, readyz, get_application_info),
    components(schemas(ApplicationInfo)),
    tags((name = "common", description = "健康探针与运行时信息"))
)]
struct CommonApiDoc;

/// 返回 common 路由的 OpenAPI 文档片段，供主 crate 合并进全局文档。
pub fn openapi() -> utoipa::openapi::OpenApi {
    CommonApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_formats_at_most_two_units() {
        assert_eq!(
            format_uptime_approx(Duration::from_secs(2 * 3600 + 15 * 60 + 30)),
            "2h 15m"
        );
    }

    #[test]
    fn static_sys_info_is_populated() {
        let (os, _arch, _host) = &*STATIC_SYS_INFO;
        assert!(!os.is_empty());
    }
}
