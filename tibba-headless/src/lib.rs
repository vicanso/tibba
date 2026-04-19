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
    #[snafu(display("{source}"))]
    HeadlessChrome { source: anyhow::Error },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::HeadlessChrome { source } => BaseError::new(source.to_string())
                .with_sub_category("headless_chrome")
                .with_status(500)
                .with_exception(true),
        };
        err.with_category("headless")
    }
}

mod chrome;

pub use chrome::*;
