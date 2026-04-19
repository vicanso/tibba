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

/// Tracing target for all log events in this crate.
/// Use `RUST_LOG=tibba:request=info` (or `debug`) to filter these logs.
pub(crate) const LOG_TARGET: &str = "tibba:request";

mod request;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{service} request fail, {message}"))]
    Common { service: String, message: String },
    #[snafu(display("{service} build http request fail, {source}"))]
    Build {
        service: String,
        source: reqwest::Error,
    },
    #[snafu(display("{service} uri fail, {source}"))]
    Uri {
        service: String,
        source: axum::http::uri::InvalidUri,
    },
    #[snafu(display("{service} http request fail, {path} {source}"))]
    Request {
        service: String,
        path: String,
        source: reqwest::Error,
    },
    #[snafu(display("{service} json fail, {source}"))]
    Serde {
        service: String,
        source: serde_json::Error,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let (service, err) = match val {
            Error::Common { service, message } => (service, BaseError::new(message)),
            Error::Build { service, source } => (service, BaseError::new(source)),
            Error::Uri { service, source } => (service, BaseError::new(source)),
            Error::Request {
                service,
                path: _,
                source,
            } => {
                let status = source.status().map_or(500, |v| v.as_u16());
                let is_network_exception = source.is_timeout() || source.is_connect();
                (
                    service,
                    BaseError::new(source)
                        .with_status(status)
                        .with_exception(is_network_exception),
                )
            }
            Error::Serde { service, source } => (service, BaseError::new(source)),
        };
        err.with_sub_category(&service).with_category("request")
    }
}

pub use request::*;
