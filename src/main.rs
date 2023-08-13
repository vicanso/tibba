use axum::{error_handling::HandleErrorLayer, middleware::from_fn_with_state, Router};
use base64_serde::base64_serde_type;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use std::{env, str::FromStr};
use tokio::signal;
use tower::ServiceBuilder;
use tracing::Level;
use tracing::{error, info};
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

async fn test() {
    #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct FamilyResult {
        pub families: Vec<String>,
    }
    #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ImageOptimParams {
        pub data: String,
        pub data_type: String,
        pub output_type: String,
        pub quality: i64,
        pub speed: i64,
    }

    base64_serde_type!(Base64Standard, base64::engine::general_purpose::STANDARD);
    #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ImageOptimResult {
        pub diff: f64,
        #[serde(with = "Base64Standard")]
        pub data: Vec<u8>,
        pub output_type: String,
        pub ratio: i64,
    }
    let result: FamilyResult = request::get_charts_instance()
        .get("/font-families")
        .await
        .unwrap();
    println!("{result:?}");

    // let result: ImageOptimResult = request::get_image_optim_instance()
    //     .post(
    //         "/optim-images",
    //         &ImageOptimParams {
    //             data: "https://img2.baidu.com/it/u=3012806272,1276873993&fm=253&fmt=auto&app=138&f=JPEG".to_string(),
    //             output_type: "avif".to_string(),
    //             quality: 90,
    //             ..Default::default()
    //         },
    //     )
    //     .await
    //     .unwrap();

    // println!("{result:?}");
}

// 检查依赖服务失败直接panic
async fn check_dependencies() {
    request::get_charts_instance();
    cache::redis_ping().await.unwrap();
}

#[tokio::main]
async fn run() {
    test().await;
    check_dependencies().await;
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
    info!("listening on http://{addr}/");
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
        error!(category = "panic", message = e.to_string(),);
    }));
    init_logger();
    run();
}
