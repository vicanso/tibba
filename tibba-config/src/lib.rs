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
use tibba_error::Error as BaseError;

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
    #[snafu(display("{category}, parse duration error {source}"))]
    ParseDuration {
        category: String,
        source: humantime::DurationError,
    },
    #[snafu(display("{category}, parse size error {source}"))]
    ParseSize {
        category: String,
        source: parse_size::Error,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let (category, err) = match val {
            Error::Url { category, source } => (category, source.to_string()),
            Error::Config { category, source } => (category, source.to_string()),
            Error::ParseDuration { category, source } => (category, source.to_string()),
            Error::ParseSize { category, source } => (category, source.to_string()),
        };

        BaseError::new(err)
            .with_sub_category(&category)
            .with_category("config")
            .with_exception(true)
    }
}

pub use app_config::*;
pub use bytesize_serde;
pub use humantime_serde;
