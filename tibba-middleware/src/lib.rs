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
        if let Some(x_forwarded_for) = parts.headers.get("X-Forwarded-For")
            && let Some(ip) = x_forwarded_for
                .to_str()
                .unwrap_or_default()
                .split(',')
                .next()
            && let Ok(ip) = ip.parse::<IpAddr>()
        {
            return Ok(ClientIp(ip));
        }
        if let Some(x_real_ip) = parts.headers.get("X-Real-Ip")
            && let Ok(ip) = x_real_ip.to_str().unwrap().parse::<IpAddr>()
        {
            return Ok(ClientIp(ip));
        }
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            .ok_or_else(|| BaseError::new("no connect info"))?;
        Ok(ClientIp(ip))
    }
}

mod common;
mod entry;
mod limit;
mod session;
mod stats;
mod tracker;

pub use common::*;
pub use entry::*;
pub use limit::*;
pub use session::*;
pub use stats::*;
pub use tracker::*;
