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
use serde::{Deserialize, Serialize};
use serde_json::json;
use snafu::ResultExt;
use snafu::Snafu;
use sqlx::{Pool, Postgres};
use std::collections::HashMap;
use tibba_error::Error as BaseError;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime};

pub const REGION_ANY: &str = "any";
pub const REGION_TX: &str = "tx";
pub const REGION_GZ: &str = "gz";
pub const REGION_ALIYUN: &str = "aliyun";

fn format_datetime(datetime: PrimitiveDateTime) -> String {
    let ts = datetime.assume_utc().unix_timestamp();
    if let Some(value) = DateTime::from_timestamp(ts, 0) {
        value.with_timezone(&offset::Local).to_string()
    } else {
        String::new()
    }
}

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

pub(crate) fn parse_primitive_datetime(s: &str) -> Result<PrimitiveDateTime> {
    let fmt_t = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    let fmt_space = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    if let Ok(dt) = PrimitiveDateTime::parse(s, fmt_t) {
        return Ok(dt);
    }
    if let Ok(dt) = PrimitiveDateTime::parse(s, fmt_space) {
        return Ok(dt);
    }
    // with timezone offset: convert to UTC then strip offset
    if let Ok(odt) = OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        let utc = odt.to_offset(time::UtcOffset::UTC);
        return Ok(PrimitiveDateTime::new(utc.date(), utc.time()));
    }
    Err(Error::InvalidDatetime {
        value: s.to_string(),
    })
}

type Result<T> = std::result::Result<T, Error>;

#[allow(async_fn_in_trait)]
pub trait Model {
    type Output: Serialize;
    fn new() -> Self;
    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView;
    fn keyword(&self) -> String {
        "name".to_string()
    }
    fn filter_condition_sql(&self, _filters: &HashMap<String, String>) -> Option<Vec<String>> {
        None
    }
    fn condition_sql(&self, params: &ModelListParams) -> Result<String> {
        let mut where_conditions = vec!["deleted_at IS NULL".to_string()];

        if let Some(keyword) = &params.keyword {
            where_conditions.push(format!("{} LIKE '%{}%'", self.keyword(), keyword));
        }

        if let Some(filters) = &params.filters {
            let filters: HashMap<String, String> =
                serde_json::from_str(filters).context(JsonSnafu)?;
            if let Some(modified) = filters.get("modified")
                && let Some((start, end)) = modified.split_once(',')
            {
                if let Ok(start) = DateTime::parse_from_rfc3339(start) {
                    where_conditions.push(format!("modified >= '{start}'"));
                }
                if let Ok(end) = DateTime::parse_from_rfc3339(end) {
                    where_conditions.push(format!("modified <= '{end}'"));
                }
            }

            if let Some(conditions) = self.filter_condition_sql(&filters) {
                where_conditions.extend(conditions);
            }
        }

        Ok(format!(" WHERE {}", where_conditions.join(" AND ")))
    }
    async fn insert(&self, _pool: &Pool<Postgres>, _params: serde_json::Value) -> Result<u64> {
        Err(Error::NotSupported {
            name: "insert".to_string(),
        })
    }
    async fn get_by_id(&self, _pool: &Pool<Postgres>, _id: u64) -> Result<Option<Self::Output>> {
        Err(Error::NotSupported {
            name: "get_by_id".to_string(),
        })
    }
    async fn delete_by_id(&self, _pool: &Pool<Postgres>, _id: u64) -> Result<()> {
        Err(Error::NotSupported {
            name: "delete_by_id".to_string(),
        })
    }
    async fn update_by_id(
        &self,
        _pool: &Pool<Postgres>,
        _id: u64,
        _params: serde_json::Value,
    ) -> Result<()> {
        Err(Error::NotSupported {
            name: "update_by_id".to_string(),
        })
    }
    async fn count(&self, _pool: &Pool<Postgres>, _params: &ModelListParams) -> Result<i64> {
        Err(Error::NotSupported {
            name: "count".to_string(),
        })
    }
    async fn list(
        &self,
        _pool: &Pool<Postgres>,
        _params: &ModelListParams,
    ) -> Result<Vec<Self::Output>> {
        Err(Error::NotSupported {
            name: "list".to_string(),
        })
    }
    async fn list_and_count(
        &self,
        pool: &Pool<Postgres>,
        count: bool,
        params: &ModelListParams,
    ) -> Result<serde_json::Value> {
        let count = if count {
            self.count(pool, params).await?
        } else {
            -1
        };
        let items = self.list(pool, params).await?;
        Ok(json!({
        "count": count,
        "items": items,
        }))
    }
    async fn search_options(
        &self,
        _pool: &Pool<Postgres>,
        _keyword: Option<String>,
    ) -> Result<Vec<SchemaOption>> {
        Ok(vec![])
    }
}

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

mod configuration;
mod detector_group;
mod detector_group_user;
mod file;
mod http_detector;
mod http_stat;
mod schema;
mod user;
mod web_page_detector;

pub use configuration::*;
pub use detector_group::*;
pub use detector_group_user::*;
pub use file::*;
pub use http_detector::*;
pub use http_stat::*;
pub use schema::*;
pub use user::*;
pub use web_page_detector::*;
