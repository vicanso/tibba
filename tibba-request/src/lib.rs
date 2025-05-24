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
use tibba_error::new_error;

mod request;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("request {service} fail, {message}"))]
    Common { service: String, message: String },
    #[snafu(display("build {service} http request fail, {source}"))]
    Build {
        service: String,
        source: reqwest::Error,
    },
    #[snafu(display("uri {service} fail, {source}"))]
    Uri {
        service: String,
        source: axum::http::uri::InvalidUri,
    },
    #[snafu(display("Http {service} request fail, {path} {source}"))]
    Request {
        service: String,
        path: String,
        source: reqwest::Error,
    },
    #[snafu(display("Json {service} fail, {source}"))]
    Serde {
        service: String,
        source: serde_json::Error,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let error_category = "request";
        match val {
            Error::Common { service, message } => new_error(message)
                .with_category(error_category)
                .with_sub_category(&service),
            Error::Build { service, source } => new_error(source)
                .with_category(error_category)
                .with_sub_category(&service),
            Error::Uri { service, source } => new_error(source)
                .with_category(error_category)
                .with_sub_category(&service),
            Error::Request {
                service,
                path: _,
                source,
            } => {
                let status = source.status().map_or(400, |v| v.as_u16());
                let exception = source.is_timeout() || source.is_request() || source.is_connect();
                new_error(source)
                    .with_category(error_category)
                    .with_sub_category(&service)
                    .with_status(status)
                    .with_exception(exception)
            }
            Error::Serde { service, source } => new_error(source)
                .with_category(error_category)
                .with_sub_category(&service),
        }
        .into()
    }
}

pub use request::*;
