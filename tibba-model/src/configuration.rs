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

use super::Model;
use super::user::{ROLE_ADMIN, ROLE_SUPER_ADMIN};
use super::{
    Error, JsonSnafu, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaType,
    SchemaView, SqlxSnafu, checked_db_id, ensure_affected, format_datetime, new_schema_options,
    parse_primitive_datetime,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct ConfigurationSchema {
    id: i64,
    status: i16,
    category: String,
    name: String,
    data: Json<serde_json::Value>,
    description: String,
    effective_start_time: PrimitiveDateTime,
    effective_end_time: PrimitiveDateTime,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Deserialize, Serialize)]
pub struct Configuration {
    pub id: i64,
    pub status: i16,
    pub category: String,
    pub name: String,
    pub data: serde_json::Value,
    pub description: String,
    pub effective_start_time: String,
    pub effective_end_time: String,
    pub created: String,
    pub modified: String,
}

impl From<ConfigurationSchema> for Configuration {
    fn from(schema: ConfigurationSchema) -> Self {
        Self {
            id: schema.id,
            status: schema.status,
            category: schema.category,
            name: schema.name,
            data: schema.data.0,
            description: schema.description,
            effective_start_time: format_datetime(schema.effective_start_time),
            effective_end_time: format_datetime(schema.effective_end_time),
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConfigurationInsertParams {
    pub category: String,
    pub name: String,
    pub data: serde_json::Value,
    pub description: Option<String>,
    pub status: i16,
    pub effective_start_time: String,
    pub effective_end_time: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConfigurationUpdateParams {
    pub data: Option<serde_json::Value>,
    pub description: Option<String>,
    pub status: Option<i16>,
    pub effective_start_time: Option<String>,
    pub effective_end_time: Option<String>,
}

fn validate_effective_range(start: PrimitiveDateTime, end: PrimitiveDateTime) -> Result<()> {
    if start > end {
        return Err(Error::Validation {
            message: "effective_start_time must not be after effective_end_time".to_string(),
        });
    }
    Ok(())
}

#[derive(Default)]
pub struct ConfigurationModel {}

impl Model for ConfigurationModel {
    type Output = Configuration;
    fn new() -> Self {
        Self::default()
    }
    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "name".to_string(),
                    category: SchemaType::String,
                    required: true,
                    read_only: true,
                    filterable: true,
                    fixed: true,
                    ..Default::default()
                },
                Schema {
                    name: "category".to_string(),
                    category: SchemaType::String,
                    required: true,
                    read_only: true,
                    filterable: true,
                    options: Some(new_schema_options(&[
                        "common",
                        "app",
                        "user",
                        "system",
                        "alarm",
                        "response_headers",
                    ])),
                    ..Default::default()
                },
                Schema::new_effective_start_time(),
                Schema::new_effective_end_time(),
                Schema {
                    name: "data".to_string(),
                    category: SchemaType::Json,
                    span: Some(2),
                    required: true,
                    popover: true,
                    ..Default::default()
                },
                Schema::new_status(),
                Schema {
                    name: "description".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema::new_created(),
                Schema::new_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                owner: true,
                roles: vec![ROLE_SUPER_ADMIN.to_string(), ROLE_ADMIN.to_string()],
                ..Default::default()
            },
            allow_create: SchemaAllowCreate {
                roles: vec![ROLE_SUPER_ADMIN.to_string(), ROLE_ADMIN.to_string()],
                ..Default::default()
            },
        }
    }

    fn push_filter_conditions(
        &self,
        qb: &mut QueryBuilder<Postgres>,
        filters: &HashMap<String, String>,
    ) -> Result<()> {
        if let Some(category) = filters.get("category") {
            qb.push(" AND category = ");
            qb.push_bind(category.clone());
        }
        Ok(())
    }
    async fn insert(&self, pool: &Pool<Postgres>, data: serde_json::Value) -> Result<u64> {
        let params: ConfigurationInsertParams = serde_json::from_value(data).context(JsonSnafu)?;
        let start = parse_primitive_datetime(&params.effective_start_time)?;
        let end = parse_primitive_datetime(&params.effective_end_time)?;
        validate_effective_range(start, end)?;
        let row: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO configurations (category, name, data, description, status, effective_start_time, effective_end_time) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id"#,
        )
        .bind(params.category)
        .bind(params.name)
        .bind(params.data)
        .bind(params.description)
        .bind(params.status)
        .bind(start)
        .bind(end)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(row.0 as u64)
    }

    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let id = checked_db_id(id)?;
        let result = sqlx::query_as::<_, ConfigurationSchema>(
            r#"SELECT * FROM configurations WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(result.map(|schema| schema.into()))
    }

    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        let id = checked_db_id(id)?;
        let result = sqlx::query(
            r#"UPDATE configurations SET deleted_at = NOW(), modified = NOW() WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;

        ensure_affected(&result)
    }

    async fn update_by_id(
        &self,
        pool: &Pool<Postgres>,
        id: u64,
        data: serde_json::Value,
    ) -> Result<()> {
        let id = checked_db_id(id)?;
        let params: ConfigurationUpdateParams = serde_json::from_value(data).context(JsonSnafu)?;
        let new_start = params
            .effective_start_time
            .as_deref()
            .map(parse_primitive_datetime)
            .transpose()?;
        let new_end = params
            .effective_end_time
            .as_deref()
            .map(parse_primitive_datetime)
            .transpose()?;
        match (new_start, new_end) {
            (Some(s), Some(e)) => validate_effective_range(s, e)?,
            (Some(_), None) | (None, Some(_)) => {
                if let Some((cur_s, cur_e)) = sqlx::query_as::<_, (PrimitiveDateTime, PrimitiveDateTime)>(
                    r#"SELECT effective_start_time, effective_end_time FROM configurations WHERE id = $1 AND deleted_at IS NULL"#,
                )
                .bind(id)
                .fetch_optional(pool)
                .await
                .context(SqlxSnafu)?
                {
                    validate_effective_range(new_start.unwrap_or(cur_s), new_end.unwrap_or(cur_e))?;
                }
            }
            (None, None) => {}
        }
        let result = sqlx::query(
            r#"UPDATE configurations SET data = COALESCE($1, data), description = COALESCE($2, description), status = COALESCE($3, status), effective_start_time = COALESCE($4, effective_start_time), effective_end_time = COALESCE($5, effective_end_time), modified = NOW() WHERE id = $6 AND deleted_at IS NULL"#,
        )
        .bind(params.data)
        .bind(params.description)
        .bind(params.status)
        .bind(new_start)
        .bind(new_end)
        .bind(id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;

        ensure_affected(&result)
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb = QueryBuilder::new("SELECT COUNT(*) FROM configurations");
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
        let mut qb = QueryBuilder::new("SELECT * FROM configurations");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb, self.orderable_columns());
        let configurations = qb
            .build_query_as::<ConfigurationSchema>()
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(configurations.into_iter().map(|s| s.into()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn configuration_preserves_json_array_and_scalar() {
        let dt = datetime!(2026-06-05 00:00:00);
        let schema = ConfigurationSchema {
            id: 1,
            status: 1,
            category: "app".to_string(),
            name: "list".to_string(),
            data: Json(serde_json::json!(["a", "b"])),
            description: String::new(),
            effective_start_time: dt,
            effective_end_time: dt,
            created: dt,
            modified: dt,
        };
        let cfg = Configuration::from(schema);
        assert_eq!(cfg.data, serde_json::json!(["a", "b"]));
    }

    #[test]
    fn effective_range_rejects_inverted_bounds() {
        let start = datetime!(2026-06-06 00:00:00);
        let end = datetime!(2026-06-05 00:00:00);
        assert!(matches!(
            validate_effective_range(start, end),
            Err(Error::Validation { .. })
        ));
        assert!(validate_effective_range(end, start).is_ok());
    }
}
