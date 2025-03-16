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

// Import required dependencies for HTTP handling, serialization, and logging
use axum::BoxError;
use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::http::{Method, Uri};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use tracing::error;

// Main Error enum that wraps HttpError
// Uses Snafu for error handling boilerplate generation
#[derive(Debug, Snafu)]
pub enum Error {
    Http { error: HttpError },
}

// Implementation of From trait to convert HttpError into Error
impl From<HttpError> for Error {
    fn from(error: HttpError) -> Self {
        Error::Http { error }
    }
}

// Represents an HTTP error with detailed information
// Implements serialization for JSON responses
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

impl HttpError {
    pub fn with_category(mut self, category: String) -> Self {
        self.category = category;
        self
    }
    pub fn with_code(mut self, code: String) -> Self {
        self.code = code;
        self
    }
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
}

pub fn new_http_error(message: String) -> HttpError {
    HttpError {
        message,
        ..Default::default()
    }
}

// Implements conversion of Error into HTTP Response
// Sets appropriate status code and headers
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let e = match self {
            Error::Http { error } => error,
        };

        let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_REQUEST);
        // for error, set no-cache
        let mut res = Json(e).into_response();
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        (status, res).into_response()
    }
}

// Creates a new Error with default status code 400 (Bad Request)
pub fn new_error(message: String) -> Error {
    new_error_with_status(message, 400)
}

// Creates a new Error with specified category
// Uses default values for other fields
pub fn new_error_with_category(message: String, category: String) -> Error {
    HttpError {
        message,
        category,
        ..Default::default()
    }
    .into()
}

// Creates a new Error with custom status code
// Uses default values for other fields
pub fn new_error_with_status(message: String, status: u16) -> Error {
    HttpError {
        message,
        status,
        ..Default::default()
    }
    .into()
}

// Creates a new Error marked as an exception with custom status code
// Sets exception flag to true
pub fn new_exception_error_with_status(message: String, status: u16) -> Error {
    HttpError {
        message,
        status,
        exception: true,
        ..Default::default()
    }
    .into()
}

// Global error handler for the application
// Processes unhandled errors and converts them into appropriate Error responses
// Handles special cases like timeout errors
pub async fn handle_error(
    method: Method, // HTTP method of the request
    uri: Uri,       // URI of the request
    err: BoxError,  // The error that occurred
) -> Error {
    // Log the error with request details
    error!("method:{}, uri:{}, error:{}", method, uri, err.to_string());

    // Special handling for timeout errors
    // Otherwise treats as internal server error (500)
    let (message, category, status) = if err.is::<tower::timeout::error::Elapsed>() {
        (
            "Request took too long".to_string(),
            "timeout".to_string(),
            408,
        )
    } else {
        (err.to_string(), "exception".to_string(), 500)
    };

    // Create and return appropriate HttpError
    HttpError {
        message,
        category,
        status,
        ..Default::default()
    }
    .into()
}
