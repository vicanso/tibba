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
use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// Main Error enum that wraps HttpError
/// Uses Snafu for error handling boilerplate generation
#[derive(Debug, Snafu, Default, Serialize, Deserialize, Clone)]
#[snafu(display("{message}"))]
pub struct Error {
    // HTTP status code
    #[serde(skip)]
    pub status: u16,
    // error category
    pub category: String,
    // error message
    pub message: String,
    // sub-category
    pub sub_category: Option<String>,
    // error code
    pub code: Option<String>,
    // whether it is an exception
    pub exception: Option<bool>,
    // other extra information
    pub extra: Option<Box<Vec<String>>>,
}

impl Error {
    #[must_use]
    pub fn new(message: impl ToString) -> Self {
        Self {
            message: message.to_string(),
            ..Default::default()
        }
    }
    /// Sets the error category
    #[must_use]
    pub fn with_category(mut self, category: impl ToString) -> Self {
        self.category = category.to_string();
        self
    }
    /// Sets the sub-category
    #[must_use]
    pub fn with_sub_category(mut self, sub_category: impl ToString) -> Self {
        self.sub_category = Some(sub_category.to_string());
        self
    }
    /// Sets the error code
    #[must_use]
    pub fn with_code(mut self, code: impl ToString) -> Self {
        self.code = Some(code.to_string());
        self
    }
    /// Sets the HTTP status code
    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
    /// Sets whether it is an exception
    #[must_use]
    pub fn with_exception(mut self, exception: bool) -> Self {
        self.exception = Some(exception);
        self
    }
    /// Adds extra information
    #[must_use]
    pub fn add_extra(mut self, value: impl ToString) -> Self {
        if self.extra.is_none() {
            self.extra = Some(Box::new(vec![]));
        }
        if let Some(extra) = self.extra.as_mut() {
            extra.push(value.to_string());
        }
        self
    }
}

/// Implements conversion of Error into HTTP Response
/// Sets appropriate status code and headers
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::BAD_REQUEST);
        // for error, set no-cache
        let mut res = Json(&self).into_response();
        res.extensions_mut().insert(self);
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        (status, res).into_response()
    }
}
