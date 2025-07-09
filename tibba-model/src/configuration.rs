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

use super::Model;
use super::user::{ROLE_ADMIN, ROLE_SUPER_ADMIN};
use super::{
    Error, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaType, SchemaView,
    Status, format_datetime, new_schema_options,
};
use async_trait::async_trait;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{MySql, Pool};
use std::collections::HashMap;
use std::str::FromStr;
use substring::Substring;
use time::OffsetDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct ConfigurationSchema {
    id: u64,
    status: i8,
    category: String,
    name: String,
    data: Json<serde_json::Value>,
    description: String,
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
    pub data: HashMap<String, serde_json::Value>,
    pub description: String,
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
            data: serde_json::from_value(schema.data.0).unwrap_or_default(),
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
    pub status: i8,
    pub effective_start_time: String,
    pub effective_end_time: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConfigurationUpdateParams {
    pub data: Option<serde_json::Value>,
    pub description: Option<String>,
    pub status: Option<i8>,
    pub effective_start_time: Option<String>,
    pub effective_end_time: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AlarmConfig {
    pub category: String,
    pub url: String,
}

#[async_trait]
impl Model for Configuration {
    type Output = Self;
    async fn schema_view(_pool: &Pool<MySql>) -> SchemaView {
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

    fn filter_condition_sql(filters: &HashMap<String, String>) -> Option<Vec<String>> {
        let mut conditions = vec![];
        if let Some(category) = filters.get("category") {
            conditions.push(format!("category = '{category}'"));
        }
        (!conditions.is_empty()).then_some(conditions)
    }
    async fn insert(pool: &Pool<MySql>, data: serde_json::Value) -> Result<u64> {
        let params: ConfigurationInsertParams =
            serde_json::from_value(data).map_err(|e| Error::Json { source: e })?;
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

    async fn get_by_id(pool: &Pool<MySql>, id: u64) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, ConfigurationSchema>(
            r#"SELECT * FROM configurations WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|schema| schema.into()))
    }

    async fn delete_by_id(pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE configurations SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }

    async fn update_by_id(pool: &Pool<MySql>, id: u64, data: serde_json::Value) -> Result<()> {
        let params: ConfigurationUpdateParams =
            serde_json::from_value(data).map_err(|e| Error::Json { source: e })?;
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

    async fn count(pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM configurations");
        sql.push_str(&Self::condition_sql(params)?);
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(count)
    }

    async fn list(pool: &Pool<MySql>, params: &ModelListParams) -> Result<Vec<Self>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM configurations");
        sql.push_str(&Self::condition_sql(params)?);
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

impl Configuration {
    pub async fn get_response_headers(pool: &Pool<MySql>, name: &str) -> Result<Option<HeaderMap>> {
        let now = OffsetDateTime::now_utc();
        let configurations = sqlx::query_as::<_, ConfigurationSchema>(
            r#"SELECT * FROM configurations 
               WHERE category = 'response_headers' 
               AND status = ?
               AND name = ? 
               AND deleted_at IS NULL
               AND effective_start_time <= ?
               AND effective_end_time >= ?"#,
        )
        .bind(Status::Enabled as i8)
        .bind(name)
        .bind(now)
        .bind(now)
        .fetch_all(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        let mut headers = HeaderMap::new();

        for configuration in configurations {
            let data = configuration.data;
            let Some(data) = data.as_object() else {
                continue;
            };
            for (key, value) in data.iter() {
                let Some(value_str) = value.as_str() else {
                    continue;
                };
                let Ok(header_value) = HeaderValue::from_str(value_str) else {
                    continue;
                };
                let Ok(header_name) = HeaderName::from_str(key) else {
                    continue;
                };
                headers.insert(header_name, header_value);
            }
        }
        Ok(Some(headers))
    }
    pub async fn get_config<T>(pool: &Pool<MySql>, category: &str, name: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let now = OffsetDateTime::now_utc();
        let configuration = sqlx::query_as::<_, ConfigurationSchema>(
            r#"SELECT * FROM configurations 
               WHERE category = ? 
               AND status = ?
               AND name = ? 
               AND deleted_at IS NULL
               AND effective_start_time <= ?
               AND effective_end_time >= ?"#,
        )
        .bind(category)
        .bind(Status::Enabled as i8)
        .bind(name)
        .bind(now)
        .bind(now)
        .fetch_one(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        let data = configuration.data;
        let Some(data) = data.as_object() else {
            return Err(Error::NotFound);
        };
        let data: T = serde_json::from_value(serde_json::Value::Object(data.clone()))
            .map_err(|e| Error::Json { source: e })?;
        Ok(data)
    }
}
