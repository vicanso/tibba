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
use std::time::Duration;
use tibba_config::Config;
use tibba_error::{Error, new_error};
use tibba_hook::register_before_task;
use tibba_session::SessionParams;
use tracing::info;
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;
static CONFIGS: OnceCell<Config> = OnceCell::new();

static SESSION_PARAMS: OnceCell<SessionParams> = OnceCell::new();

#[derive(RustEmbed)]
#[folder = "configs/"]
struct Configs;

// BasicConfig struct defines the basic application settings
// with validation rules for each field
#[derive(Debug, Clone, Default, Validate)]
pub struct BasicConfig {
    // listen address
    pub listen: String,
    // processing limit
    #[validate(range(min = 0, max = 100000))]
    pub processing_limit: i32,
    // timeout
    pub timeout: Duration,
    // secret
    pub secret: String,
    // prefix
    pub prefix: String,
    // commit id
    pub commit_id: String,
    // region
    pub region: Option<String>,
}

static BASIC_CONFIG: OnceCell<BasicConfig> = OnceCell::new();

/// Create a new basic config, if the config is invalid, it will panic
fn new_basic_config(config: &Config) -> Result<BasicConfig> {
    let timeout = config.get_duration_from_env_first("timeout", Some(Duration::from_secs(60)));
    let commit_id = if let Some(data) = Configs::get("commit_id.txt") {
        std::str::from_utf8(&data.data)
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        "--".to_string()
    };
    let region = config.get_from_env_first("region", None);
    let basic_config = BasicConfig {
        listen: config.get_from_env_first("listen", None),
        processing_limit: config.get_int_from_env_first("processing_limit", Some(5000)),
        timeout,
        secret: config.get_from_env_first("secret", None),
        prefix: config.get_from_env_first("prefix", None),
        commit_id,
        region: if region.is_empty() {
            None
        } else {
            Some(region)
        },
    };
    basic_config.validate()?;
    Ok(basic_config)
}

#[derive(Debug, Clone, Default, Validate)]
pub struct SessionConfig {
    // session ttl in seconds
    #[validate(range(min = 60, max = 2592000))]
    pub ttl_seconds: u64,
    // session secret
    #[validate(length(min = 64))]
    pub secret: String,
    // session cookie name
    #[validate(length(min = 1, max = 64))]
    pub cookie: String,
    // session max renewal
    #[validate(range(min = 1, max = 52))]
    pub max_renewal: u8,
}

static SESSION_CONFIG: OnceCell<SessionConfig> = OnceCell::new();

// Creates a new SessionConfig instance from the configuration
fn new_session_config(config: &Config) -> Result<SessionConfig> {
    let ttl = config.get_duration_from_env_first("ttl", Some(Duration::from_secs(2 * 24 * 3600)));
    let session_config = SessionConfig {
        ttl_seconds: ttl.as_secs(),
        secret: config.get_from_env_first("secret", None),
        cookie: config.get_from_env_first("cookie", None),
        max_renewal: config.get_int_from_env_first("max_renewal", Some(52)) as u8,
    };
    session_config.validate()?;
    Ok(session_config)
}

pub fn get_session_params() -> &'static SessionParams {
    SESSION_PARAMS.get_or_init(|| {
        // session config is checked in init function
        let session_config = SESSION_CONFIG.get().unwrap();

        SessionParams {
            secret: session_config.secret.clone(),
            cookie: session_config.cookie.clone(),
            ttl: session_config.ttl_seconds as i64,
            max_renewal: session_config.max_renewal,
        }
    })
}

fn new_config() -> Result<&'static Config> {
    CONFIGS.get_or_try_init(|| {
        let category = "config";
        let mut arr = vec![];
        for name in ["default.toml", &format!("{}.toml", tibba_util::get_env())] {
            let data = Configs::get(name)
                .ok_or(new_error(format!("{name} not found")).with_category(category))?
                .data;
            info!(category, "load config from {}", name);
            arr.push(std::str::from_utf8(&data).unwrap_or_default().to_string());
        }

        let config =
            tibba_config::new_config(arr.iter().map(|s| s.as_str()).collect(), Some("TIBBA_WEB"))?;
        Ok(config)
    })
}

pub fn must_get_config() -> &'static Config {
    new_config().unwrap()
}

pub fn must_get_basic_config() -> &'static BasicConfig {
    BASIC_CONFIG.get().unwrap()
}

async fn init_config() -> Result<()> {
    let app_config = new_config()?;
    let basic_config = new_basic_config(&app_config.sub_config("basic"))?;
    BASIC_CONFIG
        .set(basic_config)
        .map_err(|_| new_error("basic config init failed").with_category("config"))?;
    let session_config = new_session_config(&app_config.sub_config("session"))?;
    SESSION_CONFIG
        .set(session_config)
        .map_err(|_| new_error("session config init failed").with_category("config"))?;
    Ok(())
}

// add application init before application start
#[ctor]
fn init() {
    register_before_task(
        "application_config",
        0,
        Box::new(|| Box::pin(async { init_config().await })),
    );
}
