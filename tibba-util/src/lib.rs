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

use lz4_flex::block::DecompressError;
use snafu::Snafu;
use std::env;
use tibba_error::Error as BaseError;
use tibba_error::new_error;

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("{source}"))]
    Zstd { source: std::io::Error },
    #[snafu(display("{source}"))]
    Lz4Decompress { source: DecompressError },
    #[snafu(display("{source}"))]
    InvalidHeaderName {
        source: axum::http::header::InvalidHeaderName,
    },
    #[snafu(display("{source}"))]
    InvalidHeaderValue {
        source: axum::http::header::InvalidHeaderValue,
    },
    #[snafu(display("{source}"))]
    Axum { source: axum::Error },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let error_category = "util";
        match val {
            Error::Zstd { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("zstd"),
            Error::Lz4Decompress { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("lz4_decompress"),
            Error::InvalidHeaderName { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("invalid_header_name"),
            Error::InvalidHeaderValue { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("invalid_header_value"),
            Error::Axum { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("axum"),
        }
    }
}

pub fn get_env() -> String {
    env::var("RUST_ENV").unwrap_or_else(|_| "dev".to_string())
}

/// Whether it is a development environment
/// Used to determine whether it is a local development environment
pub fn is_development() -> bool {
    get_env() == "dev"
}

/// Whether it is a test environment
pub fn is_test() -> bool {
    get_env() == "test"
}

/// Whether it is a production environment
pub fn is_production() -> bool {
    get_env() == "production"
}

mod compression;
mod datetime;
mod http;
mod request;
mod response;
mod string;
mod value;

pub use compression::*;
pub use datetime::*;
pub use http::*;
pub use request::*;
pub use response::*;
pub use string::*;
pub use value::*;
