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
use std::time::Duration;
use tibba_error::Error;

type Result<T> = std::result::Result<T, Error>;

pub type JsonResult<T> = Result<Json<T>>;

pub struct CacheJson<T>(Duration, Json<T>);
pub type CacheJsonResult<T> = Result<CacheJson<T>>;

impl<T> From<(Duration, T)> for CacheJson<T>
where
    T: Serialize,
{
    fn from(arr: (Duration, T)) -> Self {
        CacheJson(arr.0, Json(arr.1))
    }
}

impl<T> IntoResponse for CacheJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        let secs = self.0.as_secs();
        let mut arr = vec!["public".to_string(), format!("max-age={}", secs)];
        // If the cache is too long, choose a smaller value to avoid the cache server saving data for too long
        if secs > 3600 {
            arr.push("s-maxage=3600".to_string());
        }
        ([(header::CACHE_CONTROL, arr.join(", ").as_str())], self.1).into_response()
    }
}
