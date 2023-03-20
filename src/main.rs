use axum::{error_handling::HandleErrorLayer, middleware::from_fn_with_state, Router};

use std::net::SocketAddr;
use std::time::Duration;
use tokio::signal;
use tower::ServiceBuilder;
use tracing::{debug, info};

use controller::new_router;
use middleware::{access_log, entry};
use state::get_app_state;

mod asset;
mod cache;
mod config;
mod controller;
mod error;
mod middleware;
mod state;
mod util;
async fn test() {
    // let redis_cache = cache::RedisCache::new().unwrap();

    // let lru_store = cache::TtlLruStore::new(10, Duration::from_secs(10));
    // let redis_store = cache::TtlRedisStore::new(redis_cache, Duration::from_secs(60));
    // println!("{}", chrono::Utc::now());
    // println!(
    //     "{:?}",
    //     store.set_struct("key", &HTTPError::new("def")).await
    // );

    // let result: HTTPError = store.get_struct("key").await.unwrap();
    // println!("{result:?}");
    // sleep(Duration::from_secs(12));
    // // println!("{:?}", store.set_struct("key", &HTTPError::new("测试1")));
    // let result: HTTPError = store.get_struct("key").await.unwrap();
    // println!("{result:?}");
    // sleep(Duration::from_secs(60));

    // let result: HTTPError = store.get_struct("key").await.unwrap();
    // println!("{result:?}");

    // redis_cache.set_struct("key", &HTTPError::new("测试"), None);
    // let he: HTTPError = redis_cache.get_struct("key").unwrap();
    // println!("{:?}", he);
}

#[tokio::main]
async fn main() {
    // test();

    // initialize tracing
    tracing_subscriber::fmt::init();
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
    debug!("listening on {}", addr);
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
