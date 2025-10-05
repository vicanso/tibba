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

use axum::body::{Body, Bytes};
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::header::HeaderMap;
use axum::http::request::Parts;
use axum::http::{Request, header};
use serde::de::DeserializeOwned;
use tibba_error::Error;
use validator::Validate;

#[derive(Debug, Clone)]
struct BodyBytes {
    data: Bytes,
}

fn map_err(err: impl ToString, sub_category: &str) -> Error {
    Error::new(err)
        .with_category("params")
        .with_sub_category(sub_category)
}

async fn get_body_bytes<S>(req: Request<Body>, state: &S) -> Result<Bytes, Error>
where
    S: Send + Sync,
{
    // split request
    let (mut parts, body) = req.into_parts();

    // check cache
    if let Some(cached) = parts.extensions.get::<BodyBytes>() {
        return Ok(cached.data.clone());
    }

    // create temp request
    let temp_req = Request::from_parts(parts.clone(), body);
    let body = Bytes::from_request(temp_req, state)
        .await
        .map_err(|err| map_err(err, "read_body"))?;

    // cache result
    parts.extensions.insert(BodyBytes { data: body.clone() });

    Ok(body)
}

pub struct JsonParams<T>(pub T);

impl<T, S> FromRequest<S> for JsonParams<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        if json_content_type(req.headers()) {
            let body = get_body_bytes(req, state).await?;
            let deserializer = &mut serde_json::Deserializer::from_slice(&body);

            let value: T = match serde_path_to_error::deserialize(deserializer) {
                Ok(value) => value,
                Err(err) => {
                    return Err(map_err(err, "serde_json"));
                }
            };
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
