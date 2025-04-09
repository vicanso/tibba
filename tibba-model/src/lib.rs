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

use chrono::{DateTime, offset};
use serde::Deserialize;
use snafu::ResultExt;
use snafu::Snafu;
use std::collections::HashMap;
use tibba_error::Error as BaseError;
use tibba_error::new_error;
use time::OffsetDateTime;

fn format_datetime(datetime: OffsetDateTime) -> String {
    if let Some(value) = DateTime::from_timestamp(datetime.unix_timestamp(), 0) {
        value.with_timezone(&offset::Local).to_string()
    } else {
        "".to_string()
    }
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{source}"))]
    Sqlx { source: sqlx::Error },
    #[snafu(display("{source}"))]
    Json { source: serde_json::Error },
}

impl From<Error> for BaseError {
    fn from(source: Error) -> Self {
        let error_category = "model";
        match source {
            Error::Sqlx { source } => {
                let he = new_error(&source.to_string())
                    .with_category(error_category)
                    .with_sub_category("sqlx")
                    .with_exception(true);
                he.into()
            }
            Error::Json { source } => {
                let he = new_error(&source.to_string())
                    .with_category(error_category)
                    .with_sub_category("json")
                    .with_exception(true);
                he.into()
            }
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ModelListParams {
    pub page: u64,
    pub limit: u64,
    pub order_by: Option<String>,
    pub keyword: Option<String>,
    pub filters: Option<String>,
}

impl ModelListParams {
    pub fn parse_filters(&self) -> Result<Option<HashMap<String, String>>> {
        if let Some(filters) = &self.filters {
            let filters: HashMap<String, String> =
                serde_json::from_str(filters).context(JsonSnafu)?;
            Ok(Some(filters))
        } else {
            Ok(None)
        }
    }
}

mod file;
mod schema;
mod user;

pub use file::*;
pub use schema::*;
pub use user::*;
