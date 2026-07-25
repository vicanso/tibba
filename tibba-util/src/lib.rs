// Copyright 2026 Tree xie.
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
use std::env;
use std::sync::LazyLock;
use tibba_error::Error as BaseError;

// Error 变体按 feature 门控：Zstd / Lz4Decompress 只被 compression.rs 构造，
// InvalidHeaderName / InvalidHeaderValue / Axum 只被 http.rs 构造，
// 对应 feature 关闭时变体一并消失，避免 enum 携带无法到达的分支。
#[derive(Snafu, Debug)]
pub enum Error {
    #[cfg(feature = "compression")]
    #[snafu(display("{source}"))]
    Zstd { source: std::io::Error },
    #[cfg(feature = "compression")]
    #[snafu(display("{source}"))]
    Lz4Decompress {
        source: lz4_flex::block::DecompressError,
    },
    #[cfg(feature = "http")]
    #[snafu(display("{source}"))]
    InvalidHeaderName {
        source: axum::http::header::InvalidHeaderName,
    },
    #[cfg(feature = "http")]
    #[snafu(display("{source}"))]
    InvalidHeaderValue {
        source: axum::http::header::InvalidHeaderValue,
    },
    #[cfg(feature = "http")]
    #[snafu(display("{source}"))]
    Axum { source: axum::Error },
    #[snafu(display("{message}"))]
    Invalid { message: String },
    #[snafu(display("{source}"))]
    Deserialize { source: serde_urlencoded::de::Error },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        // 单次 match：从 source 构造 BaseError 并打 sub_category，
        // 与项目其他 snafu 模块（tibba-config / tibba-sql 等）保持一致
        let err = match val {
            #[cfg(feature = "compression")]
            Error::Zstd { source } => BaseError::new(source).with_sub_category("zstd"),
            #[cfg(feature = "compression")]
            Error::Lz4Decompress { source } => {
                BaseError::new(source).with_sub_category("lz4_decompress")
            }
            #[cfg(feature = "http")]
            Error::InvalidHeaderName { source } => {
                BaseError::new(source).with_sub_category("invalid_header_name")
            }
            #[cfg(feature = "http")]
            Error::InvalidHeaderValue { source } => {
                BaseError::new(source).with_sub_category("invalid_header_value")
            }
            #[cfg(feature = "http")]
            Error::Axum { source } => BaseError::new(source).with_sub_category("axum"),
            Error::Invalid { message } => BaseError::new(message).with_sub_category("invalid"),
            Error::Deserialize { source } => {
                BaseError::new(source).with_sub_category("deserialize")
            }
        };
        err.with_category("util")
    }
}

static RUST_ENV: LazyLock<String> =
    LazyLock::new(|| env::var("RUST_ENV").unwrap_or_else(|_| "dev".to_string()));

pub fn get_env() -> &'static str {
    &RUST_ENV
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

#[cfg(feature = "compression")]
mod compression;
mod datetime;
#[cfg(feature = "http")]
mod http;
mod request;
mod response;
mod string;
mod uri;
mod validate;
mod value;

#[cfg(feature = "compression")]
pub use compression::*;
pub use datetime::*;
#[cfg(feature = "http")]
pub use http::*;
pub use request::*;
pub use response::*;
pub use string::*;
pub use uri::*;
pub use validate::*;
pub use value::*;
