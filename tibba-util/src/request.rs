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
use axum::body::Body;
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::header::HeaderMap;
use axum::http::request::Parts;
use axum::http::{Request, header};
use serde::de::DeserializeOwned;
use tibba_error::Error;
use validator::Validate;

fn map_err(err: impl ToString, sub_category: &str) -> Error {
    Error::new(err)
        .with_category("params")
        .with_sub_category(sub_category)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonParams<T>(pub T);

impl<T, S> FromRequest<S> for JsonParams<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        if json_content_type(req.headers()) {
            let Json(value) = Json::<T>::from_request(req, state)
                .await
                .map_err(|err| map_err(err, "from_json"))?;
            value.validate().map_err(|e| map_err(e, "validate"))?;

            Ok(JsonParams(value))
        } else {
            Err(map_err("Missing json content type", "from_json"))
        }
    }
}

fn json_content_type(headers: &HeaderMap) -> bool {
    let content_type = if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        content_type
    } else {
        return false;
    };

    let content_type = if let Ok(content_type) = content_type.to_str() {
        content_type
    } else {
        return false;
    };

    content_type.contains("application/json")
}

#[derive(Debug, Clone, Copy, Default)]
pub struct QueryParams<T>(pub T);

impl<T, S> FromRequestParts<S> for QueryParams<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or_default();
        let params: T =
            serde_urlencoded::from_str(query).map_err(|err| map_err(err, "from_query"))?;
        params.validate().map_err(|e| map_err(e, "validate"))?;
        Ok(QueryParams(params))
    }
}
