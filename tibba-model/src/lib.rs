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
use snafu::Snafu;
use tibba_error::Error as BaseError;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime};

pub(crate) fn format_datetime(datetime: PrimitiveDateTime) -> String {
    let ts = datetime.assume_utc().unix_timestamp();
    if let Some(value) = DateTime::from_timestamp(ts, 0) {
        value.with_timezone(&offset::Local).to_string()
    } else {
        String::new()
    }
}

pub(crate) fn parse_primitive_datetime(s: &str) -> Result<PrimitiveDateTime> {
    let fmt_t = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    let fmt_space = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    if let Ok(dt) = PrimitiveDateTime::parse(s, fmt_t) {
        return Ok(dt);
    }
    if let Ok(dt) = PrimitiveDateTime::parse(s, fmt_space) {
        return Ok(dt);
    }
    if let Ok(odt) = OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        let utc = odt.to_offset(time::UtcOffset::UTC);
        return Ok(PrimitiveDateTime::new(utc.date(), utc.time()));
    }
    Err(Error::InvalidDatetime {
        value: s.to_string(),
    })
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{source}"))]
    Sqlx { source: sqlx::Error },
    #[snafu(display("{source}"))]
    Json { source: serde_json::Error },
    #[snafu(display("Not supported function: {}", name))]
    NotSupported { name: String },
    #[snafu(display("Not found"))]
    NotFound,
    #[snafu(display("Invalid datetime: {value}"))]
    InvalidDatetime { value: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Sqlx { source } => BaseError::new(source)
                .with_sub_category("sqlx")
                .with_exception(true),
            Error::Json { source } => BaseError::new(source)
                .with_sub_category("json")
                .with_exception(true),
            Error::NotSupported { name } => {
                BaseError::new(format!("Not supported function: {name}"))
                    .with_sub_category("not_supported")
                    .with_exception(true)
            }
            Error::NotFound => BaseError::new("Not found")
                .with_sub_category("not_found")
                .with_exception(true),
            Error::InvalidDatetime { value } => {
                BaseError::new(format!("Invalid datetime: {value}"))
                    .with_sub_category("invalid_datetime")
            }
        };
        err.with_category("model")
    }
}

mod configuration;
mod detector_group;
mod detector_group_user;
mod file;
mod http_detector;
mod http_stat;
mod model;
mod schema;
mod user;
mod web_page_detector;

pub use configuration::*;
pub use detector_group::*;
pub use detector_group_user::*;
pub use file::*;
pub use http_detector::*;
pub use http_stat::*;
pub use model::*;
pub use schema::*;
pub use user::*;
pub use web_page_detector::*;
