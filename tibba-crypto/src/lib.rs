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

use snafu::Snafu;
use tibba_error::Error as BaseError;
use tibba_error::HttpError;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("hmac sha256 error {message}"))]
    HmacSha256 { message: String },
    #[snafu(display("key grip empty"))]
    KeyGripEmpty,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        match val {
            Error::HmacSha256 { message } => HttpError {
                message,
                category: "hmac_sha256".to_string(),
                ..Default::default()
            },
            Error::KeyGripEmpty => HttpError {
                message: "key grip empty".to_string(),
                category: "key_grip".to_string(),
                ..Default::default()
            },
        }
        .into()
    }
}

mod key_grip;

pub use key_grip::*;
