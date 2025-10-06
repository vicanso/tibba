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
        // get service
        let service = match &val {
            Error::Common { service, .. } => service,
            Error::Build { service, .. } => service,
            Error::Uri { service, .. } => service,
            Error::Request { service, .. } => service,
            Error::Serde { service, .. } => service,
        };

        // match error
        let err = match &val {
            Error::Request { source, .. } => {
                let status = source.status().map_or(500, |v| v.as_u16());
                let is_network_exception = source.is_timeout() || source.is_connect();
                BaseError::new(source)
                    .with_status(status)
                    .with_exception(is_network_exception)
            }
            Error::Common { message, .. } => BaseError::new(message),
            Error::Build { source, .. } => BaseError::new(source),
            Error::Uri { source, .. } => BaseError::new(source),
            Error::Serde { source, .. } => BaseError::new(source),
        };

        err.with_sub_category(service).with_category("request")
    }
}

pub use request::*;
