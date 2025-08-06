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

mod session;

pub use session::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("session id is empty"))]
    SessionIdEmpty,
    #[snafu(display("session id is invalid"))]
    SessionIdInvalid,
    #[snafu(display("session cache is not set"))]
    SessionCacheNotSet,
    #[snafu(display("{source}"))]
    Key { source: cookie::KeyError },
    #[snafu(display("session not found"))]
    SessionNotFound,
    #[snafu(display("user not login"))]
    UserNotLogin,
    #[snafu(display("user not admin"))]
    UserNotAdmin,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::SessionIdEmpty => new_error("session id is empty")
                .with_status(500)
                .with_exception(true),
            Error::SessionIdInvalid => new_error("session id is invalid")
                .with_status(500)
                .with_exception(true),
            Error::SessionCacheNotSet => new_error("session cache is not set")
                .with_status(500)
                .with_exception(true),
            Error::Key { source } => new_error(source)
                .with_sub_category("cookie")
                .with_status(500)
                .with_exception(true),
            Error::SessionNotFound => new_error("session not found")
                .with_status(500)
                .with_exception(true),
            Error::UserNotLogin => new_error("user not login")
                .with_sub_category("user")
                .with_status(401)
                .with_exception(false),
            Error::UserNotAdmin => new_error("user not admin")
                .with_sub_category("user")
                .with_status(403)
                .with_exception(false),
        };
        err.with_category("session")
    }
}
