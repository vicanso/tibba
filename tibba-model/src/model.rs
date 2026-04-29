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

use super::{Error, JsonSnafu, SchemaOption, SchemaView};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::json;
use snafu::ResultExt;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;

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

    pub fn push_pagination(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        if let Some(order_by) = &self.order_by {
            push_order_by(qb, order_by);
        }
        let limit = self.limit.min(200);
        let offset = (self.page.max(1) - 1) * limit;
        qb.push(format!(" LIMIT {limit} OFFSET {offset}"));
    }
}

/// Append a validated ORDER BY clause. Column name must be alphanumeric/underscore only.
pub fn push_order_by(qb: &mut QueryBuilder<'_, Postgres>, order_by: &str) {
    let (col, dir) = if let Some(col) = order_by.strip_prefix('-') {
        (col, "DESC")
    } else {
        (order_by, "ASC")
    };
    if col.chars().all(|c| c.is_alphanumeric() || c == '_') {
        qb.push(format!(" ORDER BY {col} {dir}"));
    }
}

#[allow(async_fn_in_trait)]
pub trait Model: Send + Sync {
    type Output: Serialize;
    fn new() -> Self;
    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView;
    fn keyword(&self) -> String {
        String::new()
    }
    fn push_filter_conditions<'args>(
        &self,
        _qb: &mut QueryBuilder<'args, Postgres>,
        _filters: &HashMap<String, String>,
    ) -> Result<()> {
        Ok(())
    }
    fn push_conditions<'args>(
        &self,
        qb: &mut QueryBuilder<'args, Postgres>,
        params: &ModelListParams,
    ) -> Result<()> {
        qb.push(" WHERE deleted_at IS NULL");

        let col = self.keyword();
        if !col.is_empty() && col.chars().all(|c| c.is_alphanumeric() || c == '_') {
            if let Some(keyword) = &params.keyword {
                qb.push(format!(" AND {col} LIKE "));
                qb.push_bind(format!("%{keyword}%"));
            }
        }

        if let Some(filters) = params.parse_filters()? {
            if let Some(modified) = filters.get("modified")
                && let Some((start, end)) = modified.split_once(',')
            {
                if let Ok(dt) = DateTime::parse_from_rfc3339(start) {
                    qb.push(" AND modified >= ");
                    qb.push_bind(dt.naive_utc());
                }
                if let Ok(dt) = DateTime::parse_from_rfc3339(end) {
                    qb.push(" AND modified <= ");
                    qb.push_bind(dt.naive_utc());
                }
            }
            self.push_filter_conditions(qb, &filters)?;
        }

        Ok(())
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
        if count {
            let (count, items) =
                tokio::try_join!(self.count(pool, params), self.list(pool, params))?;
            Ok(json!({ "count": count, "items": items }))
        } else {
            let items = self.list(pool, params).await?;
            Ok(json!({ "count": -1_i64, "items": items }))
        }
    }
    async fn search_options(
        &self,
        _pool: &Pool<Postgres>,
        _keyword: Option<String>,
    ) -> Result<Vec<SchemaOption>> {
        Ok(vec![])
    }
}
