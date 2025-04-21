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

use super::user::{ROLE_ADMIN, ROLE_SUPER_ADMIN};
use super::{
    Error, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaType, SchemaView,
    format_datetime, new_schema_options,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::{MySql, Pool};
use substring::Substring;
use time::OffsetDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct ConfigurationSchema {
    id: u64,
    status: i8,
    category: String,
    name: String,
    data: String,
    description: Option<String>,
    effective_start_time: OffsetDateTime,
    effective_end_time: OffsetDateTime,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Deserialize, Serialize)]
pub struct Configuration {
    pub id: u64,
    pub status: i8,
    pub category: String,
    pub name: String,
    pub data: String,
    pub description: Option<String>,
    pub effective_start_time: String,
    pub effective_end_time: String,
    pub created: String,
    pub modified: String,
}

impl From<ConfigurationSchema> for Configuration {
    fn from(schema: ConfigurationSchema) -> Self {
        Configuration {
            id: schema.id,
            status: schema.status,
            category: schema.category,
            name: schema.name,
            data: schema.data,
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
    pub data: String,
    pub description: Option<String>,
    pub status: i8,
    pub effective_start_time: String,
    pub effective_end_time: String,
}

impl From<serde_json::Value> for ConfigurationInsertParams {
    fn from(value: serde_json::Value) -> Self {
        ConfigurationInsertParams {
            category: value
                .get("category")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default(),
            name: value
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default(),
            data: value
                .get("data")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default(),
            description: value
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            effective_start_time: value
                .get("effective_start_time")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default(),
            effective_end_time: value
                .get("effective_end_time")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default(),
            status: value
                .get("status")
                .and_then(|v| v.as_i64())
                .map(|status| status as i8)
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConfigurationUpdateParams {
    pub data: Option<String>,
    pub description: Option<String>,
    pub status: Option<i8>,
    pub effective_start_time: Option<String>,
    pub effective_end_time: Option<String>,
}

impl From<serde_json::Value> for ConfigurationUpdateParams {
    fn from(value: serde_json::Value) -> Self {
        ConfigurationUpdateParams {
            data: value.get("data").and_then(|v| v.as_str()).map(String::from),
            description: value
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            status: value
                .get("status")
                .and_then(|v| v.as_i64())
                .map(|status| status as i8),
            effective_start_time: value
                .get("effective_start_time")
                .and_then(|v| v.as_str())
                .map(String::from),
            effective_end_time: value
                .get("effective_end_time")
                .and_then(|v| v.as_str())
                .map(String::from),
        }
    }
}

impl Configuration {
    pub fn schema_view() -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema {
                    name: "id".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    required: true,
                    hidden: true,
                    auto_create: true,
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
                        "file_headers",
                        "app",
                        "user",
                        "system",
                    ])),
                    ..Default::default()
                },
                Schema {
                    name: "name".to_string(),
                    category: SchemaType::String,
                    required: true,
                    read_only: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "effective_start_time".to_string(),
                    category: SchemaType::Date,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "effective_end_time".to_string(),
                    category: SchemaType::Date,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "data".to_string(),
                    category: SchemaType::Json,
                    span: Some(2),
                    ..Default::default()
                },
                Schema {
                    name: "status".to_string(),
                    category: SchemaType::Status,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "description".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "created".to_string(),
                    category: SchemaType::Date,
                    read_only: true,
                    hidden: true,
                    auto_create: true,
                    ..Default::default()
                },
                Schema {
                    name: "modified".to_string(),
                    category: SchemaType::Date,
                    read_only: true,
                    sortable: true,
                    auto_create: true,
                    ..Default::default()
                },
            ],
            allow_edit: SchemaAllowEdit {
                owner: true,
                groups: vec![],
                roles: vec![ROLE_SUPER_ADMIN.to_string(), ROLE_ADMIN.to_string()],
            },
            allow_create: SchemaAllowCreate {
                roles: vec![ROLE_SUPER_ADMIN.to_string(), ROLE_ADMIN.to_string()],
                ..Default::default()
            },
        }
    }

    fn condition_sql(params: &ModelListParams) -> Result<Option<String>> {
        let mut where_conditions = vec!["deleted_at IS NULL".to_string()];

        if let Some(keyword) = &params.keyword {
            where_conditions.push(format!("name LIKE '%{}%'", keyword));
        }

        if let Some(filters) = params.parse_filters()? {
            if let Some(category) = filters.get("category") {
                where_conditions.push(format!("category = '{}'", category));
            }
        }

        if !where_conditions.is_empty() {
            let sql = format!(" WHERE {}", where_conditions.join(" AND "));
            Ok(Some(sql))
        } else {
            Ok(None)
        }
    }
    pub async fn insert(pool: &Pool<MySql>, params: ConfigurationInsertParams) -> Result<u64> {
        let id = sqlx::query(
            r#"
            INSERT INTO configurations (category, name, data, description, status, effective_start_time, effective_end_time) VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(params.category)
        .bind(params.name)
        .bind(params.data)
        .bind(params.description)
        .bind(params.status)
        .bind(params.effective_start_time)
        .bind(params.effective_end_time)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(id.last_insert_id())
    }

    pub async fn get_by_id(pool: &Pool<MySql>, id: u64) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, ConfigurationSchema>(
            r#"SELECT * FROM configurations WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|schema| schema.into()))
    }

    pub async fn delete_by_id(pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE configurations SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }

    pub async fn update_by_id(
        pool: &Pool<MySql>,
        id: u64,
        params: ConfigurationUpdateParams,
    ) -> Result<()> {
        let _ = sqlx::query(
            r#"UPDATE configurations SET data = COALESCE(?, data), description = COALESCE(?, description), status = COALESCE(?, status), effective_start_time = COALESCE(?, effective_start_time), effective_end_time = COALESCE(?, effective_end_time) WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(params.data)
        .bind(params.description)
        .bind(params.status)
        .bind(params.effective_start_time)
        .bind(params.effective_end_time)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }

    pub async fn count(pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM configurations");
        if let Some(condition) = Self::condition_sql(params)? {
            sql.push_str(&condition);
        }
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(count)
    }

    pub async fn list(pool: &Pool<MySql>, params: &ModelListParams) -> Result<Vec<Self>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM configurations");
        if let Some(condition) = Self::condition_sql(params)? {
            sql.push_str(&condition);
        }
        if let Some(order_by) = &params.order_by {
            let (order_by, direction) = if order_by.starts_with("-") {
                (order_by.substring(1, order_by.len()).to_string(), "DESC")
            } else {
                (order_by.clone(), "ASC")
            };
            sql.push_str(&format!(" ORDER BY {} {}", order_by, direction));
        }
        let offset = (params.page - 1) * limit;
        sql.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

        let configurations = sqlx::query_as::<_, ConfigurationSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(configurations
            .into_iter()
            .map(|schema| schema.into())
            .collect())
    }
}
