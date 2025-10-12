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

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use snafu::Snafu;
use std::net::{IpAddr, SocketAddr};
use tibba_error::Error as BaseError;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{message}"))]
    Common { message: String, category: String },
    #[snafu(display("too many requests, limit: {limit}, current: {current}"))]
    TooManyRequests { limit: i64, current: i64 },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Common { message, category } => {
                BaseError::new(&message).with_sub_category(&category)
            }
            Error::TooManyRequests { .. } => BaseError::new(val.to_string())
                .with_sub_category("too_many_requests")
                .with_status(429),
        };
        err.with_category("middleware")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Sync,
{
    type Rejection = tibba_error::Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let client_ip = {
            // get client ip from X-Forwarded-For
            parts
                .headers
                .get("X-Forwarded-For")
                .and_then(|header| header.to_str().ok()) // convert to str
                .and_then(|s| s.split(',').next()) // get first ip
                .map(|s| s.trim()) // trim space
                .and_then(|s| s.parse::<IpAddr>().ok()) // parse ip
                // if above failed, try X-Real-Ip
                .or_else(|| {
                    parts
                        .headers
                        .get("X-Real-Ip")
                        .and_then(|header| header.to_str().ok())
                        .map(|s| s.trim())
                        .and_then(|s| s.parse::<IpAddr>().ok())
                })
                // if above failed, fallback to TCP connection info
                .or_else(|| {
                    parts
                        .extensions
                        .get::<ConnectInfo<SocketAddr>>()
                        .map(|ConnectInfo(addr)| addr.ip())
                })
        };

        // if all attempts fail (result is None), return error
        client_ip
            .map(ClientIp)
            .ok_or_else(|| BaseError::new("Client IP address could not be determined"))
    }
}

mod common;
mod entry;
mod limit;
mod stats;
mod tracker;

pub use common::*;
pub use entry::*;
pub use limit::*;
pub use stats::*;
pub use tracker::*;
