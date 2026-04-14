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

#[derive(Debug, Snafu)]
pub enum Error {
    /// Wraps the original `InvalidLength` from the hmac crate so callers can
    /// use `.context(HmacSha256Snafu)` instead of manual `map_err`.
    #[snafu(display("hmac sha256 error: {source}"))]
    HmacSha256 { source: hmac::digest::InvalidLength },

    #[snafu(display("key grip empty"))]
    KeyGripEmpty,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        // val.to_string() uses Snafu's Display, e.g. "hmac sha256 error: invalid length",
        // which is more informative than extracting source.to_string() directly.
        let message = val.to_string();
        match val {
            Error::HmacSha256 { .. } => BaseError::new(message)
                .with_category("crypto")
                .with_sub_category("hmac_sha256"),
            Error::KeyGripEmpty => BaseError::new(message)
                .with_category("crypto")
                .with_sub_category("key_grip")
                .with_status(500)
                .with_exception(true),
        }
    }
}

mod key_grip;

pub use key_grip::*;
