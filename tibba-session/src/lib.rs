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
    #[snafu(display("Session id is empty"))]
    SessionIdEmpty,
    #[snafu(display("Session cache is not set"))]
    SessionCacheNotSet,
    #[snafu(display("{source}"))]
    Key { source: cookie::KeyError },
    #[snafu(display("Session not found"))]
    SessionNotFound,
    #[snafu(display("User not login"))]
    UserNotLogin,
    #[snafu(display("User not admin"))]
    UserNotAdmin,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let error_category = "middleware";
        match val {
            Error::SessionIdEmpty => new_error("Session id is empty")
                .with_category(error_category)
                .with_sub_category("session")
                .with_status(500)
                .with_exception(true),
            Error::SessionCacheNotSet => new_error("Session cache is not set")
                .with_category(error_category)
                .with_sub_category("session")
                .with_status(500)
                .with_exception(true),
            Error::Key { source } => new_error(source)
                .with_category(error_category)
                .with_sub_category("cookie")
                .with_status(500)
                .with_exception(true),
            Error::SessionNotFound => new_error("Session not found")
                .with_category(error_category)
                .with_sub_category("session")
                .with_status(500)
                .with_exception(true),
            Error::UserNotLogin => new_error("User not login")
                .with_category(error_category)
                .with_sub_category("user")
                .with_status(401)
                .with_exception(false),
            Error::UserNotAdmin => new_error("User not admin")
                .with_category(error_category)
                .with_sub_category("user")
                .with_status(403)
                .with_exception(false),
        }
    }
}
