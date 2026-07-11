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

//! {{NAME}} 应用入口（由 tibba-scaffold 生成）。
//!
//! 对齐上游 tibba 的核心模式：
//! - hook before → AppCtx → 路由 → 中间件栈 → 优雅关闭
//! - 默认挂载 request_id / security / CORS / session / csrf / processing_limit
//! - 业务路由在 `router.rs` 中组装

use crate::router::new_router;
use axum::BoxError;
use axum::error_handling::HandleErrorLayer;
use axum::extract::DefaultBodyLimit;
use axum::http::{Method, Uri};
use axum::middleware::{from_fn, from_fn_with_state};
use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tibba_hook::{run_after_tasks, run_before_tasks};
use tibba_middleware::{
    Cors, MiddlewareOptions, SecurityHeaders, cors, entry, processing_limit, request_id,
    security_headers, stats, validate_csrf,
};
use tibba_scheduler::run_scheduler_jobs;
use tibba_session::session;
use tibba_util::{is_development, is_production};
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

/// 应用入口模块的 tracing target。
/// 可通过 `RUST_LOG={{NAME}}:app=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "{{NAME}}:app";

/// 全局请求体上限（2 MiB）。
const GLOBAL_BODY_LIMIT: usize = 2 * 1024 * 1024;

mod admin_web;
mod app_ctx;
mod cache;
mod config;
mod dal;
mod router;
mod sql;
mod state;

pub async fn handle_error(
    method: Method,
    uri: Uri,
    err: BoxError,
) -> tibba_error::Error {
    error!(method = method.to_string(), uri = uri.to_string(), err,);
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
    info!(target: LOG_TARGET, "signal received, starting graceful shutdown");
}

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

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_timer(timer)
        .with_ansi(is_development())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_before_tasks().await?;
    let ctx = app_ctx::AppCtx::install_from_globals()?;
    run_scheduler_jobs().await?;

    let basic_config = config::must_get_basic_config();
    let app = new_router(ctx)?;

    let mw = MiddlewareOptions::default();
    let mut cors_cfg = Cors::default().with_allow_credentials(basic_config.cors_allow_credentials);
    for origin in &basic_config.cors_allow_origins {
        cors_cfg = cors_cfg.add_allow_origin(origin);
    }
    if is_production() {
        cors_cfg
            .assert_production_safe()
            .map_err(|msg| -> Box<dyn std::error::Error> { msg.into() })?;
    }

    let predicate = SizeAbove::new(1024)
        .and(NotForContentType::GRPC)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE);
    let state = ctx.state;
    let session_params = Arc::new(config::get_session_params()?);

    let mut app = app.layer(
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(handle_error))
            .layer(CompressionLayer::new().compress_when(predicate))
            .timeout(basic_config.timeout)
            .layer(DefaultBodyLimit::max(GLOBAL_BODY_LIMIT))
            .layer(from_fn(request_id)),
    );
    app = app
        .layer(from_fn_with_state(
            SecurityHeaders::default().with_content_security_policy(
                "object-src 'none'; frame-ancestors 'none'; base-uri 'self'",
            ),
            security_headers,
        ))
        .layer(from_fn_with_state(Arc::new(cors_cfg), cors))
        .layer(from_fn_with_state(state, entry))
        .layer(from_fn_with_state(state, stats))
        .layer(from_fn_with_state(
            (ctx.cache, session_params.clone()),
            session,
        ));
    if mw.csrf {
        app = app.layer(from_fn(validate_csrf));
    }
    let app = app.layer(from_fn_with_state(state, processing_limit));
    state.run();

    info!(target: LOG_TARGET, "listening on http://{}/", basic_config.listen);
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
    if let Err(e) = run().await {
        error!(target: LOG_TARGET, event = "launch_app", message = ?e);
    }
    if let Err(e) = run_after_tasks().await {
        error!(target: LOG_TARGET, event = "run_after_tasks", message = ?e);
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        error!(target: LOG_TARGET, event = "panic", message = info.to_string());
        std::process::exit(1);
    }));
    init_logger();
    let cpus = std::env::var("TIBBA_THREADS")
        .map(|v| v.parse::<usize>().unwrap_or(num_cpus::get()))
        .unwrap_or(num_cpus::get())
        .max(1);
    info!(target: LOG_TARGET, threads = cpus, "starting");
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(cpus)
        .build()
        .unwrap_or_else(|e| panic!("failed to build tokio runtime: {e}"))
        .block_on(start());
}
