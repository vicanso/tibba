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
    Error, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaType, SchemaView,
    format_datetime, new_schema_options,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{MySql, Pool};
use std::collections::HashMap;
use substring::Substring;
use time::OffsetDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct HttpDetectorSchema {
    id: u64,
    status: i8,
    name: String,
    created: OffsetDateTime,
    modified: OffsetDateTime,
    url: String,
    method: Option<String>,
    alpn_protocols: Option<Json<Vec<String>>>,
    resolves: Option<Json<Vec<String>>>,
    headers: Option<Json<HashMap<String, String>>>,
    ip_version: Option<i32>,
    skip_verify: Option<bool>,
    body: Option<Vec<u8>>,
}

#[derive(Deserialize, Serialize)]
pub struct HttpDetector {
    pub id: u64,
    pub status: i8,
    pub name: String,
    pub created: String,
    pub modified: String,
    pub url: String,
    pub method: Option<String>,
    pub alpn_protocols: Option<Vec<String>>,
    pub resolves: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub ip_version: Option<i32>,
    pub skip_verify: Option<bool>,
    pub body: Option<Vec<u8>>,
}

impl From<HttpDetectorSchema> for HttpDetector {
    fn from(schema: HttpDetectorSchema) -> Self {
        Self {
            id: schema.id,
            status: schema.status,
            name: schema.name,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
            url: schema.url,
            method: schema.method,
            alpn_protocols: schema.alpn_protocols.map(|protocols| protocols.0),
            resolves: schema.resolves.map(|resolves| resolves.0),
            headers: schema.headers.map(|headers| headers.0),
            ip_version: schema.ip_version,
            skip_verify: schema.skip_verify,
            body: schema.body,
        }
    }
}

impl HttpDetector {
    pub fn schema_view() -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "name".to_string(),
                    category: SchemaType::String,
                    required: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "url".to_string(),
                    category: SchemaType::String,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "method".to_string(),
                    category: SchemaType::String,
                    options: Some(new_schema_options(&["GET", "POST", "PUT", "DELETE"])),
                    ..Default::default()
                },
                Schema {
                    name: "alpn_protocols".to_string(),
                    category: SchemaType::Strings,
                    options: Some(new_schema_options(&["http/1.1", "h2", "h3"])),
                    ..Default::default()
                },
                Schema {
                    name: "resolves".to_string(),
                    category: SchemaType::Strings,
                    ..Default::default()
                },
                Schema {
                    name: "headers".to_string(),
                    category: SchemaType::Json,
                    ..Default::default()
                },
                Schema {
                    name: "ip_version".to_string(),
                    category: SchemaType::Number,
                    ..Default::default()
                },
                Schema {
                    name: "skip_verify".to_string(),
                    category: SchemaType::Boolean,
                    ..Default::default()
                },
                Schema {
                    name: "body".to_string(),
                    category: SchemaType::Json,
                    popover: true,
                    span: Some(2),
                    ..Default::default()
                },
                Schema::new_status(),
                Schema::new_created(),
                Schema::new_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                owner: true,
                groups: vec![],
                roles: vec!["*".to_string()],
            },
            allow_create: SchemaAllowCreate {
                roles: vec!["*".to_string()],
                ..Default::default()
            },
        }
    }

    fn condition_sql(params: &ModelListParams) -> Result<String> {
        let mut where_conditions = vec!["deleted_at IS NULL".to_string()];

        if let Some(keyword) = &params.keyword {
            where_conditions.push(format!("name LIKE '%{}%'", keyword));
        }

        if let Some(filters) = params.parse_filters()? {
            if let Some(status) = filters.get("status") {
                where_conditions.push(format!("status = '{}'", status));
            }
        }

        Ok(format!(" WHERE {}", where_conditions.join(" AND ")))
    }

    pub async fn get_by_id(pool: &Pool<MySql>, id: u64) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|schema| schema.into()))
    }

    pub async fn delete_by_id(pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE http_detectors SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }

    // update by id

    pub async fn count(pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM http_detectors");
        sql.push_str(&Self::condition_sql(params)?);
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(count)
    }

    pub async fn list(pool: &Pool<MySql>, params: &ModelListParams) -> Result<Vec<Self>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM http_detectors");
        sql.push_str(&Self::condition_sql(params)?);
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

        let detectors = sqlx::query_as::<_, HttpDetectorSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
}
