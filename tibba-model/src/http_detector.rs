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
    SchemaView, format_datetime, new_schema_options,
};
use async_trait::async_trait;
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
    interval: u16,
    url: String,
    method: String,
    alpn_protocols: Option<Json<Vec<String>>>,
    resolves: Option<Json<Vec<String>>>,
    headers: Option<Json<HashMap<String, String>>>,
    ip_version: u8,
    skip_verify: bool,
    dns_servers: Option<Json<Vec<String>>>,
    body: Option<Vec<u8>>,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HttpDetector {
    pub id: u64,
    pub status: i8,
    pub name: String,
    pub interval: u16,
    pub url: String,
    pub method: String,
    pub alpn_protocols: Option<Vec<String>>,
    pub resolves: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub dns_servers: Option<Vec<String>>,
    pub ip_version: u8,
    pub skip_verify: bool,
    pub body: Option<Vec<u8>>,
    pub created: String,
    pub modified: String,
}

impl From<HttpDetectorSchema> for HttpDetector {
    fn from(schema: HttpDetectorSchema) -> Self {
        Self {
            id: schema.id,
            status: schema.status,
            name: schema.name,
            interval: schema.interval,
            url: schema.url,
            method: schema.method,
            alpn_protocols: schema.alpn_protocols.map(|protocols| protocols.0),
            resolves: schema.resolves.map(|resolves| resolves.0),
            headers: schema.headers.map(|headers| headers.0),
            dns_servers: schema.dns_servers.map(|dns_servers| dns_servers.0),
            ip_version: schema.ip_version,
            skip_verify: schema.skip_verify,
            body: schema.body,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HttpDetectorInsertParams {
    pub status: i8,
    pub name: String,
    pub url: String,
    pub method: String,
    pub alpn_protocols: Option<Vec<String>>,
    pub resolves: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub ip_version: i32,
    pub skip_verify: bool,
    pub body: Option<Vec<u8>>,
    pub interval: u16,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HttpDetectorUpdateParams {
    pub status: Option<i8>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub method: Option<String>,
    pub alpn_protocols: Option<Vec<String>>,
    pub resolves: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub ip_version: Option<i32>,
    pub skip_verify: Option<bool>,
    pub body: Option<Vec<u8>>,
    pub interval: Option<u16>,
}

impl HttpDetector {
    pub async fn list_enabled(pool: &Pool<MySql>) -> Result<Vec<Self>> {
        let detectors = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE deleted_at IS NULL AND status = 1"#,
        )
        .fetch_all(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
}

#[async_trait]
impl Model for HttpDetector {
    type Output = Self;
    async fn schema_view(_pool: &Pool<MySql>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "name".to_string(),
                    category: SchemaType::String,
                    required: true,
                    fixed: true,
                    ..Default::default()
                },
                Schema {
                    name: "interval".to_string(),
                    category: SchemaType::Number,
                    default_value: Some(serde_json::json!(1)),
                    ..Default::default()
                },
                Schema {
                    name: "url".to_string(),
                    span: Some(2),
                    category: SchemaType::String,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "method".to_string(),
                    category: SchemaType::String,
                    options: Some(new_schema_options(&["GET", "POST", "PUT", "DELETE"])),
                    default_value: Some(serde_json::json!("GET")),
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
                    default_value: Some(serde_json::json!(0)),
                    ..Default::default()
                },
                Schema {
                    name: "skip_verify".to_string(),
                    category: SchemaType::Boolean,
                    default_value: Some(serde_json::json!(false)),
                    ..Default::default()
                },
                Schema {
                    name: "dns_servers".to_string(),
                    category: SchemaType::Strings,
                    ..Default::default()
                },
                Schema {
                    name: "body".to_string(),
                    category: SchemaType::Json,
                    popover: true,
                    ..Default::default()
                },
                Schema::new_status(),
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

    fn filter_condition_sql(filters: &HashMap<String, String>) -> Option<Vec<String>> {
        let mut conditions = vec![];

        if let Some(status) = filters.get("status") {
            conditions.push(format!("status = '{}'", status));
        }

        (!conditions.is_empty()).then_some(conditions)
    }
    async fn insert(pool: &Pool<MySql>, params: serde_json::Value) -> Result<u64> {
        let params: HttpDetectorInsertParams =
            serde_json::from_value(params).map_err(|e| Error::Json { source: e })?;
        let result = sqlx::query(
            r#"INSERT INTO http_detectors (status, name, url, method, alpn_protocols, resolves, headers, ip_version, skip_verify, body, interval) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(params.status)
        .bind(params.name)
        .bind(params.url)
        .bind(params.method)
        .bind(params.alpn_protocols.map(Json).unwrap_or_default())
        .bind(params.resolves.map(Json).unwrap_or_default())
        .bind(params.headers.map(Json).unwrap_or_default())
        .bind(params.ip_version)
        .bind(params.skip_verify)
        .bind(params.body)
        .bind(params.interval)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id())
    }
    async fn get_by_id(pool: &Pool<MySql>, id: u64) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|schema| schema.into()))
    }
    async fn delete_by_id(pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE http_detectors SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }
    async fn update_by_id(pool: &Pool<MySql>, id: u64, params: serde_json::Value) -> Result<()> {
        let params: HttpDetectorUpdateParams =
            serde_json::from_value(params).map_err(|e| Error::Json { source: e })?;

        let _ = sqlx::query(
            r#"UPDATE http_detectors SET status = COALESCE(?, status), name = COALESCE(?, name), url = COALESCE(?, url), method = COALESCE(?, method), alpn_protocols = COALESCE(?, alpn_protocols), resolves = COALESCE(?, resolves), headers = COALESCE(?, headers), ip_version = COALESCE(?, ip_version), skip_verify = COALESCE(?, skip_verify), body = COALESCE(?, body), `interval` = COALESCE(?, `interval`) WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(params.status)
        .bind(params.name)
        .bind(params.url)
        .bind(params.method)
        .bind(params.alpn_protocols.map(Json).unwrap_or_default())
        .bind(params.resolves.map(Json).unwrap_or_default())
        .bind(params.headers.map(Json).unwrap_or_default())
        .bind(params.ip_version)
        .bind(params.skip_verify)
        .bind(params.body)
        .bind(params.interval)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }
    async fn count(pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM http_detectors");
        sql.push_str(&Self::condition_sql(params)?);
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(count)
    }

    async fn list(pool: &Pool<MySql>, params: &ModelListParams) -> Result<Vec<Self>> {
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
