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

use crate::config::get_config;
use axum::Router;
use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

mod config;

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

    // .with(httptrace::HTTPTraceLayer)
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_timer(timer)
        // .with_ansi(is_development())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

#[tokio::main]
async fn run() {
    let app_config = get_config();
    info!("{:?}", app_config.must_new_basic_config());
    let basic_config = app_config.must_new_basic_config();
    let app = Router::new();

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
}

fn main() {
    std::panic::set_hook(Box::new(|e| {
        // TODO 发送告警通知
        error!(category = "panic", message = e.to_string(),);
        std::process::exit(1);
    }));
    init_logger();
    info!("Starting tibba-web");
    run();
}
