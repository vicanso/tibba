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
use super::{REGION_ALIYUN, REGION_ANY, REGION_GZ, REGION_TX};
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
    script: Option<String>,
    alarm_url: String,
    random_querystring: bool,
    alarm_on_change: bool,
    retries: u8,
    failure_threshold: u8,
    regions: Json<Vec<String>>,
    group: String,
    verbose: bool,
    created_by: u64,
    remark: String,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HttpDetector {
    pub id: u64,
    pub status: i8,
    pub name: String,
    pub group: String,
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
    pub script: Option<String>,
    pub alarm_url: String,
    pub random_querystring: bool,
    pub alarm_on_change: bool,
    pub retries: u8,
    pub failure_threshold: u8,
    pub regions: Vec<String>,
    pub verbose: bool,
    pub created_by: u64,
    pub remark: String,
    pub created: String,
    pub modified: String,
}

impl From<HttpDetectorSchema> for HttpDetector {
    fn from(schema: HttpDetectorSchema) -> Self {
        Self {
            id: schema.id,
            status: schema.status,
            name: schema.name,
            group: schema.group,
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
            script: schema.script,
            alarm_url: schema.alarm_url,
            random_querystring: schema.random_querystring,
            alarm_on_change: schema.alarm_on_change,
            retries: schema.retries,
            failure_threshold: schema.failure_threshold,
            regions: schema.regions.0,
            verbose: schema.verbose,
            created_by: schema.created_by,
            remark: schema.remark,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HttpDetectorInsertParams {
    pub status: i8,
    pub name: String,
    pub group: String,
    pub url: String,
    pub method: String,
    pub alpn_protocols: Option<Vec<String>>,
    pub resolves: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub ip_version: i32,
    pub skip_verify: bool,
    pub body: Option<Vec<u8>>,
    pub script: Option<String>,
    pub alarm_url: Option<String>,
    pub interval: u16,
    pub random_querystring: bool,
    pub alarm_on_change: bool,
    pub retries: u8,
    pub failure_threshold: u8,
    pub regions: Vec<String>,
    pub verbose: bool,
    pub created_by: u64,
    pub remark: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HttpDetectorUpdateParams {
    pub status: Option<i8>,
    pub name: Option<String>,
    pub group: Option<String>,
    pub url: Option<String>,
    pub method: Option<String>,
    pub alpn_protocols: Option<Vec<String>>,
    pub resolves: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub ip_version: Option<i32>,
    pub skip_verify: Option<bool>,
    pub alarm_url: Option<String>,
    pub body: Option<Vec<u8>>,
    pub interval: Option<u16>,
    pub script: Option<String>,
    pub random_querystring: Option<bool>,
    pub alarm_on_change: Option<bool>,
    pub retries: Option<u8>,
    pub failure_threshold: Option<u8>,
    pub regions: Option<Vec<String>>,
    pub verbose: Option<bool>,
    pub remark: Option<String>,
}

pub struct HttpDetectorModel {}

impl HttpDetectorModel {
    pub async fn list_enabled(&self, pool: &Pool<MySql>) -> Result<Vec<HttpDetector>> {
        let detectors = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE deleted_at IS NULL AND status = 1"#,
        )
        .fetch_all(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
    pub async fn list_enabled_by_region(
        &self,
        pool: &Pool<MySql>,
        region: Option<String>,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<HttpDetector>> {
        let region = region.unwrap_or(REGION_ANY.to_string());
        let detectors = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE deleted_at IS NULL AND status = 1 AND (JSON_LENGTH(regions) = 0 OR JSON_CONTAINS(regions, ?) OR JSON_CONTAINS(regions, ?)) ORDER BY id ASC LIMIT ? OFFSET ?"#,
        )
        .bind(serde_json::json!(region))
        .bind(serde_json::json!(REGION_ANY.to_string()))
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
}

#[async_trait]
impl Model for HttpDetectorModel {
    type Output = HttpDetector;
    fn new() -> Self {
        Self {}
    }
    async fn schema_view(&self, _pool: &Pool<MySql>) -> SchemaView {
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
                    name: "group".to_string(),
                    category: SchemaType::String,
                    required: true,
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
                    name: "interval".to_string(),
                    category: SchemaType::Number,
                    default_value: Some(serde_json::json!(1)),
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
                    name: "regions".to_string(),
                    category: SchemaType::Strings,
                    options: Some(new_schema_options(&[
                        REGION_ANY,
                        REGION_TX,
                        REGION_GZ,
                        REGION_ALIYUN,
                    ])),
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
                    hidden_values: vec!["{}".to_string(), "[]".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "alarm_url".to_string(),
                    category: SchemaType::String,
                    popover: true,
                    ..Default::default()
                },
                Schema {
                    name: "alarm_on_change".to_string(),
                    category: SchemaType::Boolean,
                    default_value: Some(serde_json::json!(false)),
                    ..Default::default()
                },
                Schema {
                    name: "retries".to_string(),
                    category: SchemaType::Number,
                    default_value: Some(serde_json::json!(0)),
                    ..Default::default()
                },
                Schema {
                    name: "failure_threshold".to_string(),
                    category: SchemaType::Number,
                    default_value: Some(serde_json::json!(0)),
                    ..Default::default()
                },
                Schema {
                    name: "ip_version".to_string(),
                    category: SchemaType::Number,
                    default_value: Some(serde_json::json!(0)),
                    hidden_values: vec!["0".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "skip_verify".to_string(),
                    category: SchemaType::Boolean,
                    default_value: Some(serde_json::json!(false)),
                    ..Default::default()
                },
                Schema {
                    name: "random_querystring".to_string(),
                    category: SchemaType::Boolean,
                    default_value: Some(serde_json::json!(false)),
                    ..Default::default()
                },
                Schema {
                    name: "verbose".to_string(),
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
                Schema {
                    name: "script".to_string(),
                    category: SchemaType::Code,
                    span: Some(2),
                    popover: true,
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
        let params: HttpDetectorInsertParams =
            serde_json::from_value(params).map_err(|e| Error::Json { source: e })?;
        let result = sqlx::query(
            r#"INSERT INTO http_detectors (status, name, group, url, method, alpn_protocols, resolves, headers, ip_version, skip_verify, body, `interval`, script, alarm_url, random_querystring, alarm_on_change, retries, failure_threshold, verbose, regions, created_by, remark) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(params.status)
        .bind(params.name)
        .bind(params.group)
        .bind(params.url)
        .bind(params.method)
        .bind(params.alpn_protocols.map(Json).unwrap_or_default())
        .bind(params.resolves.map(Json).unwrap_or_default())
        .bind(params.headers.map(Json).unwrap_or_default())
        .bind(params.ip_version)
        .bind(params.skip_verify)
        .bind(params.body)
        .bind(params.interval)
        .bind(params.script)
        .bind(params.alarm_url.unwrap_or_default())
        .bind(params.random_querystring)
        .bind(params.alarm_on_change)
        .bind(params.retries)
        .bind(params.failure_threshold)
        .bind(params.verbose)
        .bind(Json(params.regions))
        .bind(params.created_by)
        .bind(params.remark)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id())
    }
    async fn get_by_id(&self, pool: &Pool<MySql>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|schema| schema.into()))
    }
    async fn delete_by_id(&self, pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE http_detectors SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL"#,
        )
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
        let params: HttpDetectorUpdateParams =
            serde_json::from_value(params).map_err(|e| Error::Json { source: e })?;

        let _ = sqlx::query(
            r#"UPDATE http_detectors SET status = COALESCE(?, status), name = COALESCE(?, name), group = COALESCE(?, group), url = COALESCE(?, url), method = COALESCE(?, method), alpn_protocols = COALESCE(?, alpn_protocols), resolves = COALESCE(?, resolves), headers = COALESCE(?, headers), ip_version = COALESCE(?, ip_version), skip_verify = COALESCE(?, skip_verify), body = COALESCE(?, body), `interval` = COALESCE(?, `interval`), script = COALESCE(?, script), alarm_url = COALESCE(?, alarm_url), random_querystring = COALESCE(?, random_querystring), alarm_on_change = COALESCE(?, alarm_on_change), retries = COALESCE(?, retries), failure_threshold = COALESCE(?, failure_threshold), verbose = COALESCE(?, verbose), regions = COALESCE(?, regions), remark = COALESCE(?, remark) WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(params.status)
        .bind(params.name)
        .bind(params.group)
        .bind(params.url)
        .bind(params.method)
        .bind(params.alpn_protocols.map(Json))
        .bind(params.resolves.map(Json))
        .bind(params.headers.map(Json))
        .bind(params.ip_version)
        .bind(params.skip_verify)
        .bind(params.body)
        .bind(params.interval)
        .bind(params.script)
        .bind(params.alarm_url)
        .bind(params.random_querystring)
        .bind(params.alarm_on_change)
        .bind(params.retries)
        .bind(params.failure_threshold)
        .bind(params.verbose)
        .bind(params.regions.map(Json))
        .bind(params.remark)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }
    async fn count(&self, pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM http_detectors");
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
        let mut sql = String::from("SELECT * FROM http_detectors");
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

        let detectors = sqlx::query_as::<_, HttpDetectorSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
}
