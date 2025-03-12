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
use tibba_error::{Error, new_error_with_category};
use tibba_hook::register_before_task;
use tracing::info;

static CONFIGS: OnceCell<AppConfig> = OnceCell::new();

type Result<T> = std::result::Result<T, Error>;

#[derive(RustEmbed)]
#[folder = "configs/"]
struct Configs;

fn new_config() -> Result<&'static AppConfig> {
    CONFIGS.get_or_try_init(|| {
        let mut arr = vec![];
        for name in ["default.toml", &format!("{}.toml", tibba_util::get_env())] {
            let data = Configs::get(name)
                .ok_or(new_error_with_category(
                    format!("{} not found", name),
                    "config".to_string(),
                ))?
                .data;
            info!(category = "config", "load config from {}", name);
            arr.push(std::str::from_utf8(&data).unwrap_or_default().to_string());
        }

        new_app_config(arr.iter().map(|s| s.as_str()).collect(), Some("TIBBA_WEB"))
            .map_err(|e| new_error_with_category(e.to_string(), "config".to_string()))
    })
}

pub fn must_get_config() -> &'static AppConfig {
    new_config().unwrap()
}

// add application init before application start
#[ctor]
fn init() {
    register_before_task(
        "application_config",
        0,
        Box::new(|| {
            Box::pin(async {
                new_config()?;
                Ok(())
            })
        }),
    );
}
