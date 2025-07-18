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
use tibba_error::{Error as BaseError, new_error};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("hmac sha256 error {message}"))]
    HmacSha256 { message: String },
    #[snafu(display("key grip empty"))]
    KeyGripEmpty,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let error_category = "crypto";
        match val {
            Error::HmacSha256 { message } => new_error(&message)
                .with_category(error_category)
                .with_sub_category("hmac_sha256"),
            Error::KeyGripEmpty => new_error("key grip empty")
                .with_category(error_category)
                .with_sub_category("key_grip")
                .with_status(500)
                .with_exception(true),
        }
    }
}

mod key_grip;

pub use key_grip::*;
