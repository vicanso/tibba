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

use ctor::ctor;
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use tibba_config::{AppConfig, new_app_config};
use tibba_error::{Error, new_error};
use tibba_hook::register_before_task;
use tibba_middleware::SessionParams;
use tracing::info;

type Result<T> = std::result::Result<T, Error>;
static CONFIGS: OnceCell<AppConfig> = OnceCell::new();

static SESSION_PARAMS: OnceCell<SessionParams> = OnceCell::new();

pub fn get_session_params() -> &'static SessionParams {
    SESSION_PARAMS.get_or_init(|| {
        // session config is checked in init function
        let session_config = must_get_config().new_session_config().unwrap();

        let session_prefixes = vec!["/users/".to_string(), "/files/upload".to_string()];

        SessionParams::new(session_prefixes)
            .with_secret(session_config.secret)
            .with_ttl_seconds(session_config.ttl_seconds as i64)
            .with_cookie(session_config.cookie)
    })
}

#[derive(RustEmbed)]
#[folder = "configs/"]
struct Configs;

fn new_config() -> Result<&'static AppConfig> {
    CONFIGS.get_or_try_init(|| {
        let category = "config";
        let mut arr = vec![];
        for name in ["default.toml", &format!("{}.toml", tibba_util::get_env())] {
            let data = Configs::get(name)
                .ok_or(new_error(&format!("{} not found", name)).with_category(category))?
                .data;
            info!(category, "load config from {}", name);
            arr.push(std::str::from_utf8(&data).unwrap_or_default().to_string());
        }

        new_app_config(arr.iter().map(|s| s.as_str()).collect(), Some("TIBBA_WEB"))
            .map_err(|e| new_error(&e.to_string()).with_category(category).into())
    })
}

pub fn must_get_config() -> &'static AppConfig {
    new_config().unwrap()
}

async fn check() -> Result<()> {
    let app_config = new_config()?;
    let _ = app_config.new_basic_config()?;
    let _ = app_config.new_redis_config()?;
    let _ = app_config.new_session_config()?;
    let _ = app_config.new_database_config()?;
    let _ = app_config.new_opendal_config()?;
    Ok(())
}

// add application init before application start
#[ctor]
fn init() {
    register_before_task(
        "application_config",
        0,
        Box::new(|| Box::pin(async { check().await })),
    );
}
