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

use crate::router::new_router;
use crate::state::get_app_state;
use axum::BoxError;
use axum::error_handling::HandleErrorLayer;
use axum::http::{Method, Uri};
use axum::middleware::{from_fn, from_fn_with_state};
use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tibba_hook::{run_after_tasks, run_before_tasks};
use tibba_middleware::{
    Cors, HttpCache, SecurityHeaders, cors, entry, http_cache, processing_limit, request_id,
    security_headers, stats,
};
use tibba_router_user::api_key_auth;
use tibba_scheduler::run_scheduler_jobs;
use tibba_session::session;
use tibba_util::is_development;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use opentelemetry_otlp::WithExportConfig;
use tracing::{Level, error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 应用入口模块的 tracing target。
/// 可通过 `RUST_LOG=tibba:app=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:app";

/// 进程退出时等待异步任务队列排空的上限：HTTP 优雅关闭后通知 worker 跑完在途任务，
/// 超过此时长则放弃等待，残留在途任务由 reaper 在可见性超时后重排。
const JOB_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

mod admin_web;
mod cache;
mod config;
mod dal;
mod docker;
mod feature;
mod httpstat;
mod i18n;
mod job;
mod metrics;
mod model;
mod openapi;
mod router;
mod sql;
mod state;

// Global error handler for the application
// Processes unhandled errors and converts them into appropriate Error responses
// Handles special cases like timeout errors
pub async fn handle_error(
    method: Method, // HTTP method of the request
    uri: Uri,       // URI of the request
    err: BoxError,  // The error that occurred
) -> tibba_error::Error {
    // Log the error with request details
    error!(method = method.to_string(), uri = uri.to_string(), err,);
    // error!("method:{}, uri:{}, error:{}", method, uri, err.to_string());

    // Special handling for timeout errors
    // Otherwise treats as internal server error (500)
    let (message, category, status) = if err.is::<tower::timeout::error::Elapsed>() {
        (
            "Request took too long".to_string(),
            "timeout".to_string(),
            408,
        )
    } else {
        (err.to_string(), "exception".to_string(), 500)
    };

    tibba_error::Error::new(message)
        .with_category(category)
        .with_status(status)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        // TODO 后续有需要可在此设置ping的状态
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("signal received, starting graceful shutdown");
}
/// 初始化日志 + 可选 OTLP 分布式追踪。
///
/// OTel 配置走标准环境变量（与 OTel SDK 约定一致）：
/// - `OTEL_EXPORTER_OTLP_ENDPOINT` —— OTLP collector 地址，如 `http://otel-collector:4318`。
///   缺省 / 空时 telemetry 完全关闭，仅打 fmt 日志，0 网络开销
/// - `OTEL_SERVICE_NAME` —— 资源 `service.name` 标签；缺省 `tibba`
///
/// 与既有 RUST_LOG 兼容：fmt 日志的级别仍由 RUST_LOG 决定。
fn init_logger() {
    let mut level = Level::INFO;
    if let Ok(log_level) = env::var("RUST_LOG")
        && let Ok(value) = Level::from_str(log_level.as_str())
    {
        level = value;
    }

    let timer = tracing_subscriber::fmt::time::OffsetTime::local_rfc_3339().unwrap_or_else(|_| {
        tracing_subscriber::fmt::time::OffsetTime::new(
            time::UtcOffset::UTC,
            time::format_description::well_known::Rfc3339,
        )
    });

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_timer(timer)
        .with_ansi(is_development());

    let filter = tracing_subscriber::filter::LevelFilter::from_level(level);

    // OTLP layer 由环境变量决定是否启用；endpoint 为空 → 直接 None，零开销
    let otel_layer = build_otel_layer();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();
}

/// 构造 OTLP tracing layer。配置缺失 / 构造失败时返回 None，
/// 让上游的 `.with(None)` 静默忽略——与"未启用"语义等价。
///
/// 用 HTTP/Protobuf 传输（最通用、最少 SDK 依赖）。
fn build_otel_layer<S>() -> Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    let endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty())?;
    let service_name = env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "tibba".to_string());

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(&endpoint)
        .build()
        .map_err(|e| eprintln!("otlp exporter init failed (telemetry disabled): {e}"))
        .ok()?;

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name(service_name)
                .build(),
        )
        .build();
    let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, "tibba");

    // 设为全局 provider，让 opentelemetry::global::tracer(...) 也能拿到
    opentelemetry::global::set_tracer_provider(provider);

    Some(tracing_opentelemetry::layer().with_tracer(tracer))
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_before_tasks().await?;
    run_scheduler_jobs().await?;

    // 注册异步任务 handler 并启动 worker（DB 池已在 run_before_tasks 中初始化）。
    // 并发 worker 数 = 同时执行的任务上限，按需调整。
    job::register_job_handlers()?;
    // 注册 i18n 本地化目录（按 Accept-Language 本地化错误响应 message）
    i18n::init();
    // 持有句柄以便进程退出前优雅排空在途任务（见下方 serve 之后的 shutdown）
    let job_workers = tibba_job::start(sql::get_db_pool(), 4);

    let basic_config = config::must_get_basic_config();
    let app = new_router()?;

    let predicate = SizeAbove::new(1024)
        .and(NotForContentType::GRPC)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE);
    let state = get_app_state();
    // 包成 Arc 以便 session 与 api_key_auth 两个中间件共享同一份配置
    let session_params = Arc::new(config::get_session_params()?);
    let app = app.layer(
        // service build layer execute by add order
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(handle_error))
            .layer(CompressionLayer::new().compress_when(predicate))
            .timeout(basic_config.timeout)
            // request_id 挂在最外层（仅次于错误处理 / 压缩），保证 entry / stats /
            // 业务 handler 都能从扩展中拿到 RequestId
            .layer(from_fn(request_id))
            // 安全响应头：紧随 request_id，覆盖所有正常业务响应（含静态资源 / JSON）。
            // 默认基线（HSTS / nosniff / X-Frame-Options DENY / Referrer-Policy）；
            // CSP 需按前端资源定制，故默认留空，由部署侧 with_content_security_policy 开启。
            .layer(from_fn_with_state(SecurityHeaders::default(), security_headers))
            // CORS：紧随安全头。预检（OPTIONS）在此短路返回 204，不进入下方 entry/stats/
            // session 与业务 handler。默认任意来源、无凭据；生产应 add_allow_origin 收敛来源、
            // 并按需 with_allow_credentials(true) 以支持携带 cookie 的跨域请求。
            .layer(from_fn_with_state(Arc::new(Cors::default()), cors))
            // HTTP 缓存：为 GET 响应自动生成 ETag 并处理 If-None-Match → 304。挂在压缩层
            // 内侧，对未压缩响应体计算 ETag；默认 Cache-Control: no-cache（每次带 ETag 回源校验）。
            .layer(from_fn_with_state(HttpCache::default(), http_cache))
            // i18n：按 Accept-Language 本地化错误响应 message。挂在 http_cache 内侧，
            // 使 ETag 基于本地化后的响应体计算；未注册目录 / 无匹配翻译时零开销透传。
            .layer(from_fn(tibba_i18n::i18n))
            .layer(from_fn_with_state(state, entry))
            .layer(from_fn_with_state(state, stats))
            .layer(from_fn_with_state(
                (cache::get_redis_cache(), session_params.clone()),
                session,
            ))
            // API Key 鉴权：紧随 session（更内层）。带有效 tibba_ 令牌时注入已登录
            // Session，覆盖 session 中间件放入的空 Session，使 UserSession/AdminSession
            // 提取器对 API Key 请求透明工作；无令牌/无效令牌则原样放行（按未登录处理）
            .layer(from_fn_with_state(
                (
                    sql::get_db_pool(),
                    cache::get_redis_cache(),
                    session_params.clone(),
                ),
                api_key_auth,
            ))
            .layer(from_fn_with_state(state, processing_limit)),
    );
    state.run();

    info!("listening on http://{}/", basic_config.listen);
    let listener = tokio::net::TcpListener::bind(basic_config.listen.clone()).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    // HTTP 已优雅排空（不再有新请求入队任务），再排空在途异步任务：
    // 通知 worker 停止认领、跑完手头任务后退出，最多等 JOB_DRAIN_TIMEOUT
    job_workers.shutdown(JOB_DRAIN_TIMEOUT).await;
    Ok(())
}

async fn start() {
    // only use unwrap in run function
    if let Err(e) = run().await {
        error!(target: LOG_TARGET, event = "launch_app", message = ?e)
    }
    if let Err(e) = run_after_tasks().await {
        error!(target: LOG_TARGET, event = "run_after_tasks", message = ?e);
    }
}

fn main() {
    std::panic::set_hook(Box::new(|e| {
        // TODO send alert
        error!(target: LOG_TARGET, event = "panic", message = e.to_string());
        std::process::exit(1);
    }));
    init_logger();
    let cpus = std::env::var("TIBBA_THREADS")
        .map(|v| v.parse::<usize>().unwrap_or(num_cpus::get()))
        .unwrap_or(num_cpus::get())
        .max(1);
    info!(threads = cpus, "start static server");
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(cpus)
        .build()
        .unwrap_or_else(|e| panic!("failed to build tokio runtime: {}", e))
        .block_on(start());
}
