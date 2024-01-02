use axum::extract::FromRef;
use axum::middleware::from_fn_with_state;
use axum::{error_handling::HandleErrorLayer, Router};
use axum_extra::extract::cookie::Key;
use once_cell::sync::Lazy;
use sea_orm::{ActiveModelTrait, ActiveValue};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::{env, str::FromStr};
use tokio::signal;
use tower::ServiceBuilder;
use tracing::Level;
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

use controller::new_router;
use middleware::{access_log, entry, processing_limit};
use state::get_app_state;
use util::is_development;

mod asset;
mod cache;
mod config;
mod controller;
mod db;
mod entities;
mod error;
mod httptrace;
mod keygrip;
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

    // TODO HTTPTraceLayer 需要通过trace的记录，有无可能仅针对此layer处理
    // .with(httptrace::HTTPTraceLayer)
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_timer(timer)
        .with_ansi(is_development())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

async fn test() {
    println!("{}:{}", util::uuid(), util::uuid());
    #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct DataTest {
        pub name: String,
    }

    static USER_CACHE: Lazy<cache::TwoLevelStore<DataTest>> = Lazy::new(|| {
        cache::TwoLevelStore::new(
            std::num::NonZeroUsize::new(1024).unwrap(),
            std::time::Duration::from_secs(60),
            "test:".to_string(),
        )
    });

    #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IpResult {
        pub origin: String,
    }
    let result: IpResult = request::must_get_httpbin_instance()
        .get("/ip")
        .await
        .unwrap();
    println!("{result:?}");
    let result = USER_CACHE.get("abc").await.unwrap();
    println!("{result:?}");
    USER_CACHE
        .set(
            "abc",
            DataTest {
                name: "Helloworld".to_string(),
            },
        )
        .await
        .unwrap();
    let result = USER_CACHE.get("abc").await.unwrap();
    println!("{result:?}");

    // entities::settings::ActiveModel::from_json(json)
    let data = entities::settings::ActiveModel {
        // status: ActiveValue::set(0),
        // name: ActiveValue::set("date".to_string()),
        // category: ActiveValue::set("test".to_string()),
        data: ActiveValue::set("2023-01-01".to_string()),
        remark: ActiveValue::set("测试".to_string()),
        creator: ActiveValue::set("vicanso".to_string()),
        ..Default::default()
    };

    data.save(db::get_database().await).await.unwrap();
    // data.save(db)
}

// 检查依赖服务失败直接panic
async fn check_dependencies() -> Result<(), String> {
    db::get_database()
        .await
        .ping()
        .await
        .map_err(|err| err.to_string())?;
    request::must_get_httpbin_instance();
    cache::redis_ping().await.map_err(|err| err.to_string())?;
    Ok(())
}

#[tokio::main]
async fn run() {
    // test().await;
    if let Err(err) = check_dependencies().await {
        error!(err, "check dependencies fail");
        std::process::exit(1);
    }
    let basic_config = config::must_new_basic_config();
    let app_state = get_app_state();

    // build our application with a route
    let app = Router::new()
        .merge(new_router())
        // 后面的layer先执行
        .layer(
            // service build的则是按顺序
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(error::handle_error))
                .timeout(basic_config.timeout)
                // 入口初始化(task local等
                .layer(from_fn_with_state(app_state, entry))
                // 记录访问日志
                .layer(from_fn_with_state(app_state, access_log))
                // 正在处理请求的限制
                .layer(from_fn_with_state(app_state, processing_limit)),
        );
    // TODO fall back 记录404统计

    info!("listening on http://{}/", basic_config.listen);
    let listener = tokio::net::TcpListener::bind(basic_config.listen)
        .await
        .unwrap();
    app_state.run();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    // .with_graceful_shutdown(shutdown_signal())
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
        std::process::exit(1);
    }));
    init_logger();
    run();
}
