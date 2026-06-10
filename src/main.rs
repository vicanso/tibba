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
use tibba_middleware::{entry, processing_limit, request_id, stats};
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

mod admin_web;
mod cache;
mod config;
mod dal;
mod docker;
mod httpstat;
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

    let basic_config = config::must_get_basic_config();
    let app = new_router()?;

    let predicate = SizeAbove::new(1024)
        .and(NotForContentType::GRPC)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE);
    let state = get_app_state();
    let session_params = config::get_session_params()?;
    let app = app.layer(
        // service build layer execute by add order
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(handle_error))
            .layer(CompressionLayer::new().compress_when(predicate))
            .timeout(basic_config.timeout)
            // request_id 挂在最外层（仅次于错误处理 / 压缩），保证 entry / stats /
            // 业务 handler 都能从扩展中拿到 RequestId
            .layer(from_fn(request_id))
            .layer(from_fn_with_state(state, entry))
            .layer(from_fn_with_state(state, stats))
            .layer(from_fn_with_state(
                (cache::get_redis_cache(), Arc::new(session_params)),
                session,
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
