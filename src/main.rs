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

use crate::router::new_router;
use crate::state::get_app_state;
use axum::Router;
use axum::error_handling::HandleErrorLayer;
use axum::middleware::from_fn_with_state;
use axum_client_ip::ClientIpSource;
use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tibba_error::handle_error;
use tibba_hook::{register_after_task, run_after_tasks, run_before_tasks};
use tibba_middleware::{entry, processing_limit, session, stats};
use tibba_scheduler::run_scheduler_jobs;
use tibba_util::{is_development, is_production};
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

mod cache;
mod config;
mod dal;
mod httpstat;
mod router;
mod sql;
mod state;

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
    if let Err(e) = run_after_tasks().await {
        error!(category = "run_after_tasks", message = e.to_string(),);
    }
}
fn init_logger() {
    let mut level = Level::INFO;
    if let Ok(log_level) = env::var("RUST_LOG") {
        if let Ok(value) = Level::from_str(log_level.as_str()) {
            level = value;
        }
    }

    let timer = tracing_subscriber::fmt::time::OffsetTime::local_rfc_3339().unwrap_or_else(|_| {
        tracing_subscriber::fmt::time::OffsetTime::new(
            time::UtcOffset::from_hms(0, 0, 0).unwrap(),
            time::format_description::well_known::Rfc3339,
        )
    });

    // dal::get_opendal_storage().dal.write(path, bs)

    // .with(httptrace::HTTPTraceLayer)
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_timer(timer)
        .with_ansi(is_development())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    register_after_task(
        "stop_app",
        u8::MAX,
        Box::new(|| {
            Box::pin(async {
                if !is_production() {
                    return Ok(());
                }
                // wait x seconds --> set flag --> wait y seconds
                tokio::time::sleep(Duration::from_secs(5)).await;
                get_app_state().stop();
                tokio::time::sleep(Duration::from_secs(3)).await;
                Ok(())
            })
        }),
    );

    run_before_tasks().await?;
    run_scheduler_jobs().await?;

    // config is validated in init function
    let basic_config = config::must_get_basic_config();
    let app = if basic_config.prefix.is_empty() {
        new_router()?
    } else {
        Router::new().nest(&basic_config.prefix, new_router()?)
    };

    let predicate = SizeAbove::new(1024)
        .and(NotForContentType::GRPC)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE);
    let state = get_app_state();
    let app = app.layer(
        // service build layer execute by add order
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(handle_error))
            .layer(CompressionLayer::new().compress_when(predicate))
            .timeout(basic_config.timeout)
            // TODO 使用 RightmostXForwardedFor 代替 ConnectInfo
            .layer(ClientIpSource::ConnectInfo.into_extension())
            .layer(from_fn_with_state(state, entry))
            .layer(from_fn_with_state(state, stats))
            .layer(from_fn_with_state(
                (
                    cache::get_redis_cache(),
                    config::get_session_params(),
                    ["/files/preview"]
                        .iter()
                        .map(|s| format!("{}{}", basic_config.prefix, s))
                        .collect(),
                ),
                session,
            ))
            .layer(from_fn_with_state(state, processing_limit)),
    );
    state.run();

    info!("listening on http://{}/", basic_config.listen);
    let listener = tokio::net::TcpListener::bind(basic_config.listen.clone())
        .await
        .unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();
    Ok(())
}

#[tokio::main]
async fn start() {
    // only use unwrap in run function
    if let Err(e) = run().await {
        error!(category = "launch_app", message = e.to_string(),);
        return;
    }
}

fn main() {
    std::panic::set_hook(Box::new(|e| {
        // TODO send alert
        error!(category = "panic", message = e.to_string(),);
        std::process::exit(1);
    }));
    init_logger();
    start();
}
