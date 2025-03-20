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

use crate::config::must_get_config;
use crate::router::new_router;
use crate::state::get_app_state;
use axum::Router;
use axum::error_handling::HandleErrorLayer;
use axum::middleware::from_fn_with_state;
use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use tibba_error::handle_error;
use tibba_hook::run_before_tasks;
use tibba_middleware::{entry, processing_limit, session, stats};
use tower::ServiceBuilder;
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

mod cache;
mod config;
mod router;
mod sql;
mod state;

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

    // .with(httptrace::HTTPTraceLayer)
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_timer(timer)
        // .with_ansi(is_development())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_before_tasks().await?;

    let app_config = must_get_config();
    // config is validated in init function
    let basic_config = app_config.new_basic_config()?;

    let app = Router::new().merge(new_router()?).layer(
        // service build layer execute by add order
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(handle_error))
            .timeout(basic_config.timeout)
            .layer(from_fn_with_state(get_app_state(), entry))
            .layer(from_fn_with_state(get_app_state(), stats))
            .layer(from_fn_with_state(
                (cache::get_redis_cache(), config::get_session_params()),
                session,
            ))
            .layer(from_fn_with_state(get_app_state(), processing_limit)),
    );
    get_app_state().run();

    info!("listening on http://{}/", basic_config.listen);
    let listener = tokio::net::TcpListener::bind(basic_config.listen)
        .await
        .unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    // .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();
    Ok(())
}

#[tokio::main]
async fn start() {
    // let client = ClientBuilder::new("baidu")
    //     .with_base_url("https://baidu.com")
    //     .with_timeout(std::time::Duration::from_secs(10))
    //     .with_common_interceptor()
    //     .build()
    //     .unwrap();
    // let resp = client
    //     .request_raw(Params::<Vec<(&str, &str)>, ()> {
    //         url: "/?a=eYtFiWeWeq",
    //         method: axum::http::Method::GET,
    //         query: Some(&vec![("b", "123")]),
    //         ..Default::default()
    //     })
    //     .await
    //     .unwrap();
    // info!("response body size: {}", resp.len());

    // only use unwrap in run function
    if let Err(e) = run().await {
        error!(category = "run", message = e.to_string(),);
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
