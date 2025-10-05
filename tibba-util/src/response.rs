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
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::fmt::Write;
use std::time::Duration;
use tibba_error::Error;

type Result<T> = std::result::Result<T, Error>;

pub type JsonResult<T> = Result<Json<T>>;

const S_MAX_AGE_LIMIT_SECS: u64 = 3600;

pub struct CacheJson<T> {
    pub duration: Duration,
    pub data: T,
}

pub type CacheJsonResult<T> = Result<CacheJson<T>>;

impl<T> From<(Duration, T)> for CacheJson<T> {
    fn from(value: (Duration, T)) -> Self {
        Self {
            duration: value.0,
            data: value.1,
        }
    }
}

impl<T> IntoResponse for CacheJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        let secs = self.duration.as_secs();

        // use `write!` macro to build Header string efficiently, avoid multiple memory allocations.
        // pre-allocate a reasonable capacity to further improve performance.
        let mut cache_control_value = String::with_capacity(64);

        // `write!` macro can write formatted content directly into String, more efficient than `format!` and `push_str`.
        // because writing to String will not fail.
        let _ = write!(&mut cache_control_value, "public, max-age={secs}");

        if secs > S_MAX_AGE_LIMIT_SECS {
            let _ = write!(
                &mut cache_control_value,
                ", s-maxage={S_MAX_AGE_LIMIT_SECS}"
            );
        }

        // finally wrap the data into `Json`.
        (
            [(header::CACHE_CONTROL, cache_control_value)],
            Json(self.data),
        )
            .into_response()
    }
}
