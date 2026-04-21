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
    DetectorGroupModel, Error, JsonSnafu, Model, ModelListParams, Schema, SchemaAllowCreate,
    SchemaAllowEdit, SchemaOption, SchemaOptionValue, SchemaType, SchemaView, SqlxSnafu,
    format_datetime, new_schema_options,
};
use super::{REGION_ALIYUN, REGION_ANY, REGION_GZ, REGION_TX};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use std::collections::HashMap;
use substring::Substring;
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct HttpDetectorSchema {
    id: i64,
    status: i16,
    name: String,
    interval: i16,
    url: String,
    method: String,
    alpn_protocols: Option<Json<Vec<String>>>,
    resolves: Option<Json<Vec<String>>>,
    headers: Option<Json<HashMap<String, String>>>,
    ip_version: i16,
    skip_verify: bool,
    dns_servers: Option<Json<Vec<String>>>,
    body: Option<Vec<u8>>,
    script: Option<String>,
    alarm_url: String,
    random_querystring: bool,
    alarm_on_change: bool,
    retries: i16,
    failure_threshold: i16,
    regions: Json<Vec<String>>,
    group_id: i64,
    verbose: bool,
    created_by: i64,
    remark: String,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HttpDetector {
    pub id: i64,
    pub status: i16,
    pub name: String,
    pub group_id: i64,
    pub interval: i16,
    pub url: String,
    pub method: String,
    pub alpn_protocols: Option<Vec<String>>,
    pub resolves: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub dns_servers: Option<Vec<String>>,
    pub ip_version: i16,
    pub skip_verify: bool,
    pub body: Option<Vec<u8>>,
    pub script: Option<String>,
    pub alarm_url: String,
    pub random_querystring: bool,
    pub alarm_on_change: bool,
    pub retries: i16,
    pub failure_threshold: i16,
    pub regions: Vec<String>,
    pub verbose: bool,
    pub created_by: i64,
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
            group_id: schema.group_id,
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
    pub status: i16,
    pub name: String,
    pub group_id: u64,
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
    pub status: Option<i16>,
    pub name: Option<String>,
    pub group_id: Option<u64>,
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
    pub async fn list_enabled(&self, pool: &Pool<Postgres>) -> Result<Vec<HttpDetector>> {
        let detectors = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE deleted_at IS NULL AND status = 1"#,
        )
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
    pub async fn list_enabled_by_region(
        &self,
        pool: &Pool<Postgres>,
        region: Option<String>,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<HttpDetector>> {
        let region = region.unwrap_or(REGION_ANY.to_string());
        let detectors = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE deleted_at IS NULL AND status = 1 AND (jsonb_array_length(regions) = 0 OR regions @> $1::jsonb OR regions @> $2::jsonb) ORDER BY id ASC LIMIT $3 OFFSET $4"#,
        )
        .bind(format!("[{:?}]", region))
        .bind(format!("[{:?}]", REGION_ANY))
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
}

#[async_trait]
impl Model for HttpDetectorModel {
    type Output = HttpDetector;
    fn new() -> Self {
        Self {}
    }
    async fn schema_view(&self, pool: &Pool<Postgres>) -> SchemaView {
        let mut group_options = vec![];
        let group_model = DetectorGroupModel {};
        if let Ok(groups) = group_model.list_enabled(pool).await {
            for group in groups {
                group_options.push(SchemaOption {
                    label: group.name,
                    value: SchemaOptionValue::Integer(group.id),
                });
            }
            group_options.sort_by_key(|option| option.label.clone());
        }
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
                    name: "group_id".to_string(),
                    category: SchemaType::Number,
                    required: true,
                    options: Some(group_options),
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
                    default_value: Some(serde_json::json!(5)),
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
            conditions.push(format!("status = {status}"));
        }

        (!conditions.is_empty()).then_some(conditions)
    }
    async fn insert(&self, pool: &Pool<Postgres>, params: serde_json::Value) -> Result<u64> {
        let params: HttpDetectorInsertParams = serde_json::from_value(params).context(JsonSnafu)?;
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO http_detectors (status, name, group_id, url, method, alpn_protocols, resolves, headers, ip_version, skip_verify, body, interval, script, alarm_url, random_querystring, alarm_on_change, retries, failure_threshold, verbose, regions, created_by, remark) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22) RETURNING id"#,
        )
        .bind(params.status)
        .bind(params.name)
        .bind(params.group_id as i64)
        .bind(params.url)
        .bind(params.method)
        .bind(params.alpn_protocols.map(Json).unwrap_or_default())
        .bind(params.resolves.map(Json).unwrap_or_default())
        .bind(params.headers.map(Json).unwrap_or_default())
        .bind(params.ip_version as i16)
        .bind(params.skip_verify)
        .bind(params.body)
        .bind(params.interval as i16)
        .bind(params.script)
        .bind(params.alarm_url.unwrap_or_default())
        .bind(params.random_querystring)
        .bind(params.alarm_on_change)
        .bind(params.retries as i16)
        .bind(params.failure_threshold as i16)
        .bind(params.verbose)
        .bind(Json(params.regions))
        .bind(params.created_by as i64)
        .bind(params.remark)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(row.0 as u64)
    }
    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, HttpDetectorSchema>(
            r#"SELECT * FROM http_detectors WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(result.map(|schema| schema.into()))
    }
    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE http_detectors SET deleted_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
        )
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
        let params: HttpDetectorUpdateParams = serde_json::from_value(params).context(JsonSnafu)?;

        let _ = sqlx::query(
            r#"UPDATE http_detectors SET status = COALESCE($1, status), name = COALESCE($2, name), group_id = COALESCE($3, group_id), url = COALESCE($4, url), method = COALESCE($5, method), alpn_protocols = COALESCE($6, alpn_protocols), resolves = COALESCE($7, resolves), headers = COALESCE($8, headers), ip_version = COALESCE($9, ip_version), skip_verify = COALESCE($10, skip_verify), body = COALESCE($11, body), interval = COALESCE($12, interval), script = COALESCE($13, script), alarm_url = COALESCE($14, alarm_url), random_querystring = COALESCE($15, random_querystring), alarm_on_change = COALESCE($16, alarm_on_change), retries = COALESCE($17, retries), failure_threshold = COALESCE($18, failure_threshold), verbose = COALESCE($19, verbose), regions = COALESCE($20, regions), remark = COALESCE($21, remark) WHERE id = $22 AND deleted_at IS NULL"#,
        )
        .bind(params.status)
        .bind(params.name)
        .bind(params.group_id.map(|v| v as i64))
        .bind(params.url)
        .bind(params.method)
        .bind(params.alpn_protocols.map(Json))
        .bind(params.resolves.map(Json))
        .bind(params.headers.map(Json))
        .bind(params.ip_version.map(|v| v as i16))
        .bind(params.skip_verify)
        .bind(params.body)
        .bind(params.interval.map(|v| v as i16))
        .bind(params.script)
        .bind(params.alarm_url)
        .bind(params.random_querystring)
        .bind(params.alarm_on_change)
        .bind(params.retries.map(|v| v as i16))
        .bind(params.failure_threshold.map(|v| v as i16))
        .bind(params.verbose)
        .bind(params.regions.map(Json))
        .bind(params.remark)
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(())
    }
    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM http_detectors");
        sql.push_str(&self.condition_sql(params)?);
        let count = sqlx::query_scalar::<_, i64>(&sql)
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
            .context(SqlxSnafu)?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
}
