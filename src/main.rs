use axum::{error_handling::HandleErrorLayer, middleware::from_fn_with_state, Router};
use std::net::SocketAddr;
use std::time::Duration;
use std::{env, str::FromStr};
use tokio::signal;
use tower::ServiceBuilder;
use tracing::{info, error};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use controller::new_router;
use middleware::{access_log, entry};
use state::get_app_state;
use util::is_development;

mod asset;
mod cache;
mod config;
mod controller;
mod error;
mod middleware;
mod request;
mod state;
mod task_local;
mod util;

fn init_logger() {
    let mut level = Level::INFO;
    if let Ok(log_level) = env::var("LOG_LEVEL") {
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

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_timer(timer)
        .with_ansi(is_development())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

#[tokio::main]
async fn run() {
    let app_state = get_app_state();

    // build our application with a route
    let app = Router::new()
        .merge(new_router())
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(error::handle_error))
                .timeout(Duration::from_secs(30)),
        )
        // 后面的layer先执行
        .layer(from_fn_with_state(app_state, access_log))
        .layer(from_fn_with_state(app_state, entry));

    let basic_config = config::must_new_basic_config();

    let addr = basic_config.listen.parse().unwrap();
    info!("listening on {addr}");
    app_state.run();
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        get_app_state().stop();
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

fn main() {
    std::panic::set_hook(Box::new(|e| {
        // TODO 发送告警通知
        error!(
            category = "panic",
            message = e.to_string(),
        );
    }));
    init_logger();
    run();
}
