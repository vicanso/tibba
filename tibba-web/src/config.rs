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

use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use tibba_config::{AppConfig, new_app_config};

static CONFIGS: OnceCell<AppConfig> = OnceCell::new();

#[derive(RustEmbed)]
#[folder = "configs/"]
struct Configs;

fn new_config() -> &'static AppConfig {
    CONFIGS.get_or_init(|| {
        let config = Configs::get("default.toml").unwrap().data;
        let app_config = new_app_config(
            vec![std::str::from_utf8(&config).unwrap_or_default()],
            Some("TIBBA_WEB"),
        );
        app_config
    })
}

pub fn get_config() -> &'static AppConfig {
    new_config()
}
