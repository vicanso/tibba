// Copyright 2026 Tree xie.
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

pub fn format_datetime(datetime: PrimitiveDateTime) -> String {
    let ts = datetime.assume_utc().unix_timestamp();
    if let Some(value) = DateTime::from_timestamp(ts, 0) {
        value.with_timezone(&offset::Local).to_string()
    } else {
        String::new()
    }
}

/// 返回当前 UTC 时刻的 `PrimitiveDateTime`，用于与 SQL 端 timestamp 类型比较。
/// 之前 `get_response_headers` / `get_config` 各写了一份相同的构造逻辑，统一在此。
pub fn now_primitive_utc() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

pub fn parse_primitive_datetime(s: &str) -> Result<PrimitiveDateTime> {
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
#[snafu(visibility(pub))]
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
    #[snafu(display("Insufficient balance"))]
    InsufficientBalance,
    #[snafu(display("{message}"))]
    InvalidAmount { message: String },
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
            Error::InsufficientBalance => BaseError::new("Insufficient balance")
                .with_sub_category("insufficient_balance")
                .with_status(402),
            Error::InvalidAmount { message } => BaseError::new(message)
                .with_sub_category("invalid_amount")
                .with_status(400),
        };
        err.with_category("model")
    }
}

mod configuration;
mod model;
mod schema;
mod user;

pub use configuration::*;
pub use model::*;
pub use schema::*;
pub use user::*;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use time::macros::datetime;

    #[test]
    fn parse_t_separator_format() {
        let dt = parse_primitive_datetime("2026-06-05T12:34:56").unwrap();
        assert_eq!(dt, datetime!(2026-06-05 12:34:56));
    }

    #[test]
    fn parse_space_separator_format() {
        let dt = parse_primitive_datetime("2026-06-05 12:34:56").unwrap();
        assert_eq!(dt, datetime!(2026-06-05 12:34:56));
    }

    #[test]
    fn parse_rfc3339_converts_to_utc() {
        // +08:00 时刻 → UTC 应减 8 小时
        let dt = parse_primitive_datetime("2026-06-05T12:34:56+08:00").unwrap();
        assert_eq!(dt, datetime!(2026-06-05 04:34:56));
    }

    #[test]
    fn parse_invalid_returns_error() {
        let err = parse_primitive_datetime("not a date").unwrap_err();
        assert!(matches!(err, Error::InvalidDatetime { ref value } if value == "not a date"));
    }

    #[test]
    fn now_primitive_utc_returns_close_to_chrono_now() {
        // 与 chrono 系统时钟比较，验证「现在」差距在 1 秒内（CI 慢机也够用）
        let ours = now_primitive_utc();
        let chrono_now = chrono::Utc::now();
        let ours_ts = ours.assume_utc().unix_timestamp();
        let diff = (ours_ts - chrono_now.timestamp()).abs();
        assert!(
            diff <= 1,
            "now_primitive_utc 应与系统时钟一致，差距 {diff}s"
        );
    }
}
