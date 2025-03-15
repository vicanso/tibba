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
use snafu::Snafu;

mod app_config;

// Error enum for handling various error types in the configuration
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{category}, url parse error {source}"))]
    Url {
        category: String,
        source: url::ParseError,
    },
    #[snafu(display("{category}, config error {source}"))]
    Config {
        category: String,
        source: config::ConfigError,
    },
    #[snafu(display("{category}, validate error {source}"))]
    Validate {
        category: String,
        source: validator::ValidationErrors,
    },
    #[snafu(display("{category}, parse duration error {source}"))]
    ParseDuration {
        category: String,
        source: humantime::DurationError,
    },
}

pub use app_config::*;
