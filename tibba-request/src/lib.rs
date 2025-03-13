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
use tibba_error::HttpError;

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
        match val {
            Error::Common { service, message } => HttpError {
                message,
                category: service,
                ..Default::default()
            },
            Error::Build { service, source } => HttpError {
                message: source.to_string(),
                category: service,
                ..Default::default()
            },
            Error::Uri { service, source } => HttpError {
                message: source.to_string(),
                category: service,
                ..Default::default()
            },
            Error::Request {
                service,
                path: _,
                source,
            } => HttpError {
                message: source.to_string(),
                category: service,
                ..Default::default()
            },
            Error::Serde { service, source } => HttpError {
                message: source.to_string(),
                category: service,
                ..Default::default()
            },
        }
        .into()
    }
}

pub use request::*;
