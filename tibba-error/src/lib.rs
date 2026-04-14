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

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

/// Rarely-set optional fields, boxed to keep `Error` small.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ErrorData {
    pub sub_category: Option<String>,
    pub code: Option<String>,
    pub exception: Option<bool>,
    pub extra: Option<Vec<String>>,
}

// Private view used only for serializing Error as a flat JSON object.
#[derive(Serialize)]
struct ErrorSerialize<'a> {
    category: &'a str,
    message: &'a str,
    #[serde(flatten)]
    data: &'a ErrorData,
}

// Private view used only for deserializing Error from a flat JSON object.
#[derive(Deserialize)]
struct ErrorDeserialize {
    #[serde(default)]
    category: String,
    #[serde(default)]
    message: String,
    #[serde(flatten)]
    data: ErrorData,
}

/// HTTP error type used throughout the application.
///
/// `category` and `message` are always present and sit directly on the struct.
/// Optional fields are grouped in a `Box<ErrorData>` so the `Err`-variant in
/// `Result<T, Error>` stays well under the 128-byte `result_large_err` limit.
#[derive(Debug, Clone, Default)]
pub struct Error {
    /// HTTP status code (0 means unset → falls back to 500 in `IntoResponse`).
    pub status: u16,
    pub category: String,
    pub message: String,
    data: Box<ErrorData>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

/// Serializes as a flat JSON object: `{ category, message, sub_category?, … }`.
impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorSerialize {
            category: &self.category,
            message: &self.message,
            data: &self.data,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Error {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let d = ErrorDeserialize::deserialize(deserializer)?;
        Ok(Self {
            status: 0,
            category: d.category,
            message: d.message,
            data: Box::new(d.data),
        })
    }
}

/// Exposes the optional fields (`sub_category`, `code`, `exception`, `extra`)
/// directly on `Error` via `Deref`/`DerefMut`.
impl Deref for Error {
    type Target = ErrorData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for Error {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Error {
    #[must_use]
    pub fn new(message: impl ToString) -> Self {
        Self {
            message: message.to_string(),
            ..Default::default()
        }
    }
    #[must_use]
    pub fn with_category(mut self, category: impl ToString) -> Self {
        self.category = category.to_string();
        self
    }
    #[must_use]
    pub fn with_sub_category(mut self, sub_category: impl ToString) -> Self {
        self.sub_category = Some(sub_category.to_string());
        self
    }
    #[must_use]
    pub fn with_code(mut self, code: impl ToString) -> Self {
        self.code = Some(code.to_string());
        self
    }
    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
    #[must_use]
    pub fn with_exception(mut self, exception: bool) -> Self {
        self.exception = Some(exception);
        self
    }
    #[must_use]
    pub fn add_extra(mut self, value: impl ToString) -> Self {
        self.extra
            .get_or_insert_with(Vec::new)
            .push(value.to_string());
        self
    }
}

/// Converts `Error` into an HTTP response with JSON body and `no-cache` header.
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        // for error, set no-cache
        let mut res = (status, Json(&self)).into_response();
        res.extensions_mut().insert(self);
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        res
    }
}
