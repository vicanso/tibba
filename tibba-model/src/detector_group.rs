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

use super::{
    Error, Model, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaType,
    SchemaView, format_datetime,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::{MySql, Pool};
use std::collections::HashMap;
use substring::Substring;
use time::OffsetDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct DetectorGroupSchema {
    id: u64,
    name: String,
    code: String,
    description: String,
    owner_id: u64,
    status: i8,
    remark: String,
    created_by: u64,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DetectorGroup {
    pub id: u64,
    pub name: String,
    pub code: String,
    pub description: String,
    pub owner_id: u64,
    pub status: i8,
    pub created_by: u64,
    pub remark: String,
    pub created: String,
    pub modified: String,
}

impl From<DetectorGroupSchema> for DetectorGroup {
    fn from(schema: DetectorGroupSchema) -> Self {
        Self {
            id: schema.id,
            name: schema.name,
            code: schema.code,
            description: schema.description,
            owner_id: schema.owner_id,
            status: schema.status,
            created_by: schema.created_by,
            remark: schema.remark,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DetectorGroupInsertParams {
    pub name: String,
    pub code: String,
    pub description: String,
    pub owner_id: u64,
    pub status: i8,
    pub remark: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DetectorGroupUpdateParams {
    pub name: Option<String>,
    pub description: Option<String>,
    pub owner_id: Option<u64>,
    pub status: Option<i8>,
    pub remark: Option<String>,
}

pub struct DetectorGroupModel {}

impl DetectorGroupModel {
    pub async fn list_enabled(&self, pool: &Pool<MySql>) -> Result<Vec<DetectorGroup>> {
        let groups = sqlx::query_as::<_, DetectorGroupSchema>(
            r#"SELECT * FROM detector_groups WHERE deleted_at IS NULL AND status = 1"#,
        )
        .fetch_all(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(groups.into_iter().map(|schema| schema.into()).collect())
    }
}

#[async_trait]
impl Model for DetectorGroupModel {
    type Output = DetectorGroup;
    fn new() -> Self {
        Self {}
    }
    async fn schema_view(&self, _pool: &Pool<MySql>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "code".to_string(),
                    category: SchemaType::String,
                    required: true,
                    fixed: true,
                    ..Default::default()
                },
                Schema {
                    name: "name".to_string(),
                    category: SchemaType::String,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "description".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "owner_id".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema::new_status(),
                Schema::new_remark(),
                Schema::new_created(),
                Schema::new_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                owner: true,
                roles: vec!["*".to_string()],
                ..Default::default()
            },
            allow_create: SchemaAllowCreate {
                roles: vec!["*".to_string()],
                ..Default::default()
            },
        }
    }

    fn filter_condition_sql(&self, filters: &HashMap<String, String>) -> Option<Vec<String>> {
        let mut conditions = vec![];

        if let Some(status) = filters.get("status") {
            conditions.push(format!("status = '{status}'"));
        }

        (!conditions.is_empty()).then_some(conditions)
    }

    async fn insert(&self, pool: &Pool<MySql>, params: serde_json::Value) -> Result<u64> {
        let params: DetectorGroupInsertParams =
            serde_json::from_value(params).map_err(|e| Error::Json { source: e })?;
        let result = sqlx::query(
            r#"INSERT INTO detector_groups (name, code, description, owner_id, status, remark) VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(params.name)
        .bind(params.code)
        .bind(params.description)
        .bind(params.owner_id)
        .bind(params.status)
        .bind(params.remark)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id())
    }

    async fn get_by_id(&self, pool: &Pool<MySql>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, DetectorGroupSchema>(
            r#"SELECT * FROM detector_groups WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|schema| schema.into()))
    }

    async fn delete_by_id(&self, pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(r#"UPDATE detector_groups SET deleted_at = NOW() WHERE id = ?"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }

    async fn update_by_id(
        &self,
        pool: &Pool<MySql>,
        id: u64,
        params: serde_json::Value,
    ) -> Result<()> {
        let params: DetectorGroupUpdateParams =
            serde_json::from_value(params).map_err(|e| Error::Json { source: e })?;

        let _ = sqlx::query(
            r#"UPDATE detector_groups SET name = COALESCE(?, name), description = COALESCE(?, description), owner_id = COALESCE(?, owner_id), status = COALESCE(?, status), remark = COALESCE(?, remark) WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(params.name)
        .bind(params.description)
        .bind(params.owner_id)
        .bind(params.status)
        .bind(params.remark)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }

    async fn count(&self, pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM detector_groups");
        sql.push_str(&self.condition_sql(params)?);
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(count)
    }

    async fn list(
        &self,
        pool: &Pool<MySql>,
        params: &ModelListParams,
    ) -> Result<Vec<Self::Output>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM detector_groups");
        sql.push_str(&self.condition_sql(params)?);
        if let Some(order_by) = &params.order_by {
            let (order_by, direction) = if order_by.starts_with("-") {
                (order_by.substring(1, order_by.len()).to_string(), "DESC")
            } else {
                (order_by.clone(), "ASC")
            };
            sql.push_str(&format!(" ORDER BY {order_by} {direction}"));
        }
        let offset = (params.page - 1) * limit;
        sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));

        let groups = sqlx::query_as::<_, DetectorGroupSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(groups.into_iter().map(|schema| schema.into()).collect())
    }
}
