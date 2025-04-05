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
    #[snafu(display("{message}"))]
    Common { message: String, category: String },
    #[snafu(display("Too many requests, limit: {limit}, current: {current}"))]
    TooManyRequests { limit: i64, current: i64 },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let error_category = "middleware";
        match val {
            Error::Common { message, category } => new_error(&message)
                .with_category(error_category)
                .with_sub_category(&category),
            Error::TooManyRequests { limit, current } => new_error(&format!(
                "Too many requests, limit: {limit}, current: {current}"
            ))
            .with_category(error_category)
            .with_sub_category("too_many_requests")
            .with_status(429),
        }
        .into()
    }
}

mod common;
mod entry;
mod limit;
mod session;
mod stats;

pub use common::*;
pub use entry::*;
pub use limit::*;
pub use session::*;
pub use stats::*;
