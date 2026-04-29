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
    Error, JsonSnafu, Model, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit,
    SchemaType, SchemaView, SqlxSnafu, format_datetime,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct DetectorGroupSchema {
    id: i64,
    name: String,
    code: String,
    owner_id: i64,
    status: i16,
    remark: String,
    created_by: i64,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DetectorGroup {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub owner_id: i64,
    pub status: i16,
    pub created_by: i64,
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
    pub owner_id: u64,
    pub created_by: u64,
    pub status: i16,
    pub remark: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DetectorGroupUpdateParams {
    pub name: Option<String>,
    pub owner_id: Option<u64>,
    pub status: Option<i16>,
    pub remark: Option<String>,
}

pub struct DetectorGroupModel {}

impl DetectorGroupModel {
    pub async fn list_enabled(&self, pool: &Pool<Postgres>) -> Result<Vec<DetectorGroup>> {
        let groups = sqlx::query_as::<_, DetectorGroupSchema>(
            r#"SELECT * FROM detector_groups WHERE deleted_at IS NULL AND status = 1"#,
        )
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(groups.into_iter().map(|schema| schema.into()).collect())
    }
}

impl Model for DetectorGroupModel {
    type Output = DetectorGroup;
    fn new() -> Self {
        Self {}
    }
    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
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
                    name: "owner_id".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    auto_create: true,
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

    fn push_filter_conditions<'args>(
        &self,
        qb: &mut QueryBuilder<'args, Postgres>,
        filters: &HashMap<String, String>,
    ) -> Result<()> {
        if let Some(status) = filters.get("status").and_then(|s| s.parse::<i16>().ok()) {
            qb.push(" AND status = ");
            qb.push_bind(status);
        }
        Ok(())
    }

    async fn insert(&self, pool: &Pool<Postgres>, params: serde_json::Value) -> Result<u64> {
        let params: DetectorGroupInsertParams =
            serde_json::from_value(params).context(JsonSnafu)?;
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO detector_groups (name, code, owner_id, created_by, status, remark) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id"#,
        )
        .bind(params.name)
        .bind(params.code)
        .bind(params.owner_id as i64)
        .bind(params.created_by as i64)
        .bind(params.status)
        .bind(params.remark)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(row.0 as u64)
    }

    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, DetectorGroupSchema>(
            r#"SELECT * FROM detector_groups WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(result.map(|schema| schema.into()))
    }

    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        sqlx::query(r#"UPDATE detector_groups SET deleted_at = NOW() WHERE id = $1"#)
            .bind(id as i64)
            .execute(pool)
            .await
            .context(SqlxSnafu)?;

        Ok(())
    }

    async fn update_by_id(
        &self,
        pool: &Pool<Postgres>,
        id: u64,
        params: serde_json::Value,
    ) -> Result<()> {
        let params: DetectorGroupUpdateParams =
            serde_json::from_value(params).context(JsonSnafu)?;

        let _ = sqlx::query(
            r#"UPDATE detector_groups SET name = COALESCE($1, name), owner_id = COALESCE($2, owner_id), status = COALESCE($3, status), remark = COALESCE($4, remark) WHERE id = $5 AND deleted_at IS NULL"#,
        )
        .bind(params.name)
        .bind(params.owner_id.map(|v| v as i64))
        .bind(params.status)
        .bind(params.remark)
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb = QueryBuilder::new("SELECT COUNT(*) FROM detector_groups");
        self.push_conditions(&mut qb, params)?;
        let count = qb
            .build_query_scalar::<i64>()
            .fetch_one(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(count)
    }

    async fn list(
        &self,
        pool: &Pool<Postgres>,
        params: &ModelListParams,
    ) -> Result<Vec<Self::Output>> {
        let mut qb = QueryBuilder::new("SELECT * FROM detector_groups");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb);
        let groups = qb
            .build_query_as::<DetectorGroupSchema>()
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(groups.into_iter().map(|s| s.into()).collect())
    }
}
