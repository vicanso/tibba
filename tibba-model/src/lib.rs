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

use async_trait::async_trait;
use chrono::{DateTime, offset};
use serde::Deserialize;
use snafu::ResultExt;
use snafu::Snafu;
use sqlx::{MySql, Pool};
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
    #[snafu(display("Not supported function: {}", name))]
    NotSupported { name: String },
}

impl From<Error> for BaseError {
    fn from(source: Error) -> Self {
        let error_category = "model";
        match source {
            Error::Sqlx { source } => {
                let he = new_error(source)
                    .with_category(error_category)
                    .with_sub_category("sqlx")
                    .with_exception(true);
                he.into()
            }
            Error::Json { source } => {
                let he = new_error(source)
                    .with_category(error_category)
                    .with_sub_category("json")
                    .with_exception(true);
                he.into()
            }
            Error::NotSupported { name } => {
                let he = new_error(format!("Not supported function: {name}"))
                    .with_category(error_category)
                    .with_sub_category("not_supported")
                    .with_exception(true);
                he.into()
            }
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

#[async_trait]
pub trait Model {
    type Output;
    async fn schema_view(_pool: &Pool<MySql>) -> SchemaView;
    fn keyword() -> String {
        "name".to_string()
    }
    fn filter_condition_sql(_filters: &HashMap<String, String>) -> Option<Vec<String>> {
        None
    }
    fn condition_sql(params: &ModelListParams) -> Result<String> {
        let mut where_conditions = vec!["deleted_at IS NULL".to_string()];

        if let Some(keyword) = &params.keyword {
            where_conditions.push(format!("{} LIKE '%{}%'", Self::keyword(), keyword));
        }

        if let Some(filters) = &params.filters {
            let filters: HashMap<String, String> =
                serde_json::from_str(filters).context(JsonSnafu)?;
            if let Some(modified) = filters.get("modified") {
                if let Some((start, end)) = modified.split_once(',') {
                    if let Ok(start) = DateTime::parse_from_rfc3339(start) {
                        where_conditions.push(format!("modified >= '{start}'"));
                    }
                    if let Ok(end) = DateTime::parse_from_rfc3339(end) {
                        where_conditions.push(format!("modified <= '{end}'"));
                    }
                }
            }

            if let Some(conditions) = Self::filter_condition_sql(&filters) {
                where_conditions.extend(conditions);
            }
        }

        Ok(format!(" WHERE {}", where_conditions.join(" AND ")))
    }
    async fn insert(_pool: &Pool<MySql>, _params: serde_json::Value) -> Result<u64> {
        Err(Error::NotSupported {
            name: "insert".to_string(),
        })
    }
    async fn get_by_id(_pool: &Pool<MySql>, _id: u64) -> Result<Option<Self::Output>> {
        Err(Error::NotSupported {
            name: "get_by_id".to_string(),
        })
    }
    async fn delete_by_id(_pool: &Pool<MySql>, _id: u64) -> Result<()> {
        Err(Error::NotSupported {
            name: "delete_by_id".to_string(),
        })
    }
    async fn update_by_id(_pool: &Pool<MySql>, _id: u64, _params: serde_json::Value) -> Result<()> {
        Err(Error::NotSupported {
            name: "update_by_id".to_string(),
        })
    }
    async fn count(_pool: &Pool<MySql>, _params: &ModelListParams) -> Result<i64> {
        Err(Error::NotSupported {
            name: "count".to_string(),
        })
    }
    async fn list(_pool: &Pool<MySql>, _params: &ModelListParams) -> Result<Vec<Self::Output>> {
        Err(Error::NotSupported {
            name: "list".to_string(),
        })
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
mod file;
mod http_detector;
mod http_stat;
mod schema;
mod user;

pub use configuration::*;
pub use file::*;
pub use http_detector::*;
pub use http_stat::*;
pub use schema::*;
pub use user::*;
