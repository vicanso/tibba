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

use axum::BoxError;
use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::http::{Method, Uri};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use tracing::error;

#[derive(Debug, Snafu)]
pub enum Error {
    Config {
        source: tibba_config::Error,
    },
    Http {
        error: HttpError,
    },
    #[snafu(display("{message}"))]
    Invalid {
        message: String,
    },
}

impl From<HttpError> for Error {
    fn from(error: HttpError) -> Self {
        Error::Http { error }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HttpError {
    // error message
    pub message: String,
    // error category
    pub category: String,
    // error code
    pub code: String,
    // HTTP status code
    pub status: u16,
    // whether it is an exception
    pub exception: bool,
    // other extra information
    pub extra: Option<Vec<String>>,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let e = match self {
            Error::Http { error } => error,
            Error::Config { source } => HttpError {
                category: "config".to_string(),
                status: 500,
                message: source.to_string(),
                exception: true,
                ..Default::default()
            },
            Error::Invalid { message } => HttpError {
                category: "invalid".to_string(),
                status: 400,
                message,
                exception: true,
                ..Default::default()
            },
        };

        let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_REQUEST);
        // for error, set no-cache
        let mut res = Json(e).into_response();
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        (status, res).into_response()
    }
}

pub fn new_error(message: String) -> Error {
    new_error_with_status(message, 400)
}

pub fn new_error_with_status(message: String, status: u16) -> Error {
    HttpError {
        message,
        status,
        ..Default::default()
    }
    .into()
}

pub async fn handle_error(
    // `Method` and `Uri` are extractors so they can be used here
    method: Method,
    uri: Uri,
    // the last argument must be the error itself
    err: BoxError,
) -> Error {
    error!("method:{}, uri:{}, error:{}", method, uri, err.to_string());
    let (message, category, status) = if err.is::<tower::timeout::error::Elapsed>() {
        (
            "Request took too long".to_string(),
            "timeout".to_string(),
            408,
        )
    } else {
        (err.to_string(), "exception".to_string(), 500)
    };
    HttpError {
        message,
        category,
        status,
        ..Default::default()
    }
    .into()
}
