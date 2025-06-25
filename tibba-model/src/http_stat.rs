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
    Error, HttpDetector, Model, ModelListParams, ResultValue, Schema, SchemaAllowCreate,
    SchemaAllowEdit, SchemaOption, SchemaOptionValue, SchemaType, SchemaView, format_datetime,
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
struct HttpStatSchema {
    id: u64,
    target_id: u64,
    target_name: String,
    url: String,
    dns_lookup: i32,
    quic_connect: i32,
    tcp_connect: i32,
    tls_handshake: i32,
    server_processing: i32,
    content_transfer: i32,
    total: i32,
    addr: String,
    status_code: u16,
    tls: String,
    alpn: String,
    subject: String,
    issuer: String,
    cert_not_before: String,
    cert_not_after: String,
    cert_cipher: String,
    cert_domains: String,
    body_size: i32,
    error: String,
    result: u8,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Default, Deserialize, Serialize, Debug, Clone)]
pub struct HttpStat {
    pub id: u64,
    pub target_id: u64,
    pub target_name: String,
    pub url: String,
    pub dns_lookup: i32,
    pub quic_connect: i32,
    pub tcp_connect: i32,
    pub tls_handshake: i32,
    pub server_processing: i32,
    pub content_transfer: i32,
    pub total: i32,
    pub addr: String,
    pub status_code: u16,
    pub tls: String,
    pub alpn: String,
    pub subject: String,
    pub issuer: String,
    pub cert_not_before: String,
    pub cert_not_after: String,
    pub cert_cipher: String,
    pub cert_domains: Vec<String>,
    pub body_size: i32,
    pub error: String,
    pub result: u8,
    pub created: String,
    pub modified: String,
}

impl From<HttpStatSchema> for HttpStat {
    fn from(schema: HttpStatSchema) -> Self {
        Self {
            id: schema.id,
            target_id: schema.target_id,
            target_name: schema.target_name,
            url: schema.url,
            dns_lookup: schema.dns_lookup,
            quic_connect: schema.quic_connect,
            tcp_connect: schema.tcp_connect,
            tls_handshake: schema.tls_handshake,
            server_processing: schema.server_processing,
            content_transfer: schema.content_transfer,
            total: schema.total,
            addr: schema.addr,
            status_code: schema.status_code,
            tls: schema.tls,
            alpn: schema.alpn,
            subject: schema.subject,
            issuer: schema.issuer,
            cert_not_before: schema.cert_not_before,
            cert_not_after: schema.cert_not_after,
            cert_cipher: schema.cert_cipher,
            cert_domains: schema
                .cert_domains
                .split(',')
                .map(|s| s.to_string())
                .collect(),
            body_size: schema.body_size,
            error: schema.error,
            result: schema.result,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]

pub struct HttpStatInsertParams {
    pub target_id: u64,
    pub target_name: String,
    pub url: String,
    pub dns_lookup: Option<i32>,
    pub quic_connect: Option<i32>,
    pub tcp_connect: Option<i32>,
    pub tls_handshake: Option<i32>,
    pub server_processing: Option<i32>,
    pub content_transfer: Option<i32>,
    pub total: Option<i32>,
    pub addr: String,
    pub status_code: Option<u16>,
    pub tls: Option<String>,
    pub alpn: Option<String>,
    pub subject: Option<String>,
    pub issuer: Option<String>,
    pub cert_not_before: Option<String>,
    pub cert_not_after: Option<String>,
    pub cert_cipher: Option<String>,
    pub cert_domains: Option<String>,
    pub body_size: Option<i32>,
    pub error: Option<String>,
    pub result: u8,
}

impl HttpStat {
    pub async fn add_stat(pool: &Pool<MySql>, params: HttpStatInsertParams) -> Result<u64> {
        let result = sqlx::query(
            r#"INSERT INTO http_stats (target_id, target_name, url, dns_lookup, quic_connect, tcp_connect, tls_handshake, server_processing, content_transfer, total, addr, status_code, tls, alpn, subject, issuer, cert_not_before, cert_not_after, cert_cipher, cert_domains, body_size, error, result) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(params.target_id)
        .bind(params.target_name)
        .bind(params.url)
        .bind(params.dns_lookup.unwrap_or(-1))
        .bind(params.quic_connect.unwrap_or(-1))
        .bind(params.tcp_connect.unwrap_or(-1))
        .bind(params.tls_handshake.unwrap_or(-1))
        .bind(params.server_processing.unwrap_or(-1))
        .bind(params.content_transfer.unwrap_or(-1))
        .bind(params.total.unwrap_or(-1))
        .bind(params.addr)
        .bind(params.status_code.unwrap_or(0))
        .bind(params.tls.unwrap_or_default())
        .bind(params.alpn.unwrap_or_default())
        .bind(params.subject.unwrap_or_default())
        .bind(params.issuer.unwrap_or_default())
        .bind(params.cert_not_before.unwrap_or_default())
        .bind(params.cert_not_after.unwrap_or_default())
        .bind(params.cert_cipher.unwrap_or_default())
        .bind(params.cert_domains.unwrap_or_default())
        .bind(params.body_size.unwrap_or(-1))
        .bind(params.error.unwrap_or_default())
        .bind(params.result)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id())
    }
    pub async fn list_by_modified(
        pool: &Pool<MySql>,
        modified_range: (&str, &str),
    ) -> Result<Vec<Self>> {
        let detectors = sqlx::query_as::<_, HttpStatSchema>(
            r#"SELECT * FROM http_stats WHERE modified >= ? AND modified <= ?"#,
        )
        .bind(modified_range.0)
        .bind(modified_range.1)
        .fetch_all(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;
        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
}

#[async_trait]
impl Model for HttpStat {
    type Output = Self;
    fn keyword() -> String {
        "target_name".to_string()
    }
    async fn schema_view(_pool: &Pool<MySql>) -> SchemaView {
        let mut detector_options = vec![];
        if let Ok(detectors) = HttpDetector::list_enabled(_pool).await {
            for detector in detectors {
                detector_options.push(SchemaOption {
                    label: detector.name,
                    value: SchemaOptionValue::String(detector.id.to_string()),
                });
            }
        }
        SchemaView {
            schemas: vec![
                Schema {
                    name: "target_id".to_string(),
                    category: SchemaType::String,
                    hidden: true,
                    filterable: !detector_options.is_empty(),
                    options: Some(detector_options),
                    ..Default::default()
                },
                Schema {
                    name: "target_name".to_string(),
                    label: Some("name".to_string()),
                    category: SchemaType::String,
                    fixed: true,
                    ..Default::default()
                },
                Schema {
                    name: "url".to_string(),
                    category: SchemaType::String,
                    max_width: Some(200),
                    ..Default::default()
                },
                Schema {
                    name: "result".to_string(),
                    category: SchemaType::Result,
                    filterable: true,
                    options: Some(vec![
                        SchemaOption {
                            label: "Success".to_string(),
                            value: SchemaOptionValue::String(
                                (ResultValue::Success as u8).to_string(),
                            ),
                        },
                        SchemaOption {
                            label: "Failed".to_string(),
                            value: SchemaOptionValue::String(
                                (ResultValue::Failed as u8).to_string(),
                            ),
                        },
                    ]),
                    ..Default::default()
                },
                Schema {
                    name: "total".to_string(),
                    category: SchemaType::Number,
                    hidden_values: vec!["-1".to_string()],
                    filterable: true,
                    options: Some(vec![
                        SchemaOption {
                            label: ">= 1s".to_string(),
                            value: SchemaOptionValue::String("1000".to_string()),
                        },
                        SchemaOption {
                            label: ">= 2s".to_string(),
                            value: SchemaOptionValue::String("2000".to_string()),
                        },
                        SchemaOption {
                            label: ">= 3s".to_string(),
                            value: SchemaOptionValue::String("3000".to_string()),
                        },
                    ]),
                    ..Default::default()
                },
                Schema {
                    name: "dns_lookup".to_string(),
                    category: SchemaType::Number,
                    hidden_values: vec!["-1".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "quic_connect".to_string(),
                    category: SchemaType::Number,
                    hidden_values: vec!["-1".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "tcp_connect".to_string(),
                    category: SchemaType::Number,
                    hidden_values: vec!["-1".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "tls_handshake".to_string(),
                    category: SchemaType::Number,
                    hidden_values: vec!["-1".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "server_processing".to_string(),
                    category: SchemaType::Number,
                    hidden_values: vec!["-1".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "content_transfer".to_string(),
                    category: SchemaType::Number,
                    hidden_values: vec!["-1".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "timing".to_string(),
                    category: SchemaType::HoverCard,
                    combinations: Some(vec![
                        "dns_lookup".to_string(),
                        "quic_connect".to_string(),
                        "tcp_connect".to_string(),
                        "tls_handshake".to_string(),
                        "server_processing".to_string(),
                        "content_transfer".to_string(),
                    ]),
                    ..Default::default()
                },
                Schema {
                    name: "addr".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "status_code".to_string(),
                    category: SchemaType::Number,
                    hidden_values: vec!["0".to_string()],
                    ..Default::default()
                },
                Schema {
                    name: "tls".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "alpn".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "subject".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "issuer".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "cert_not_before".to_string(),
                    category: SchemaType::Date,
                    ..Default::default()
                },
                Schema {
                    name: "cert_not_after".to_string(),
                    category: SchemaType::Date,
                    ..Default::default()
                },
                Schema {
                    name: "cert_cipher".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "cert_domains".to_string(),
                    category: SchemaType::Strings,
                    ..Default::default()
                },
                Schema {
                    name: "body_size".to_string(),
                    category: SchemaType::ByteSize,
                    ..Default::default()
                },
                Schema {
                    name: "error".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema::new_created(),
                Schema::new_filterable_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                disabled: true,
                ..Default::default()
            },
            allow_create: SchemaAllowCreate {
                disabled: true,
                ..Default::default()
            },
        }
    }

    fn filter_condition_sql(filters: &HashMap<String, String>) -> Option<Vec<String>> {
        let mut conditions = vec![];
        if let Some(result) = filters.get("result") {
            conditions.push(format!("result = '{}'", result));
        }
        if let Some(target_id) = filters.get("target_id") {
            conditions.push(format!("target_id = '{}'", target_id));
        }
        if let Some(total) = filters.get("total") {
            conditions.push(format!("total >= {}", total));
        }
        (!conditions.is_empty()).then_some(conditions)
    }

    async fn list(pool: &Pool<MySql>, params: &ModelListParams) -> Result<Vec<Self>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM http_stats");
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

        let detectors = sqlx::query_as::<_, HttpStatSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
    async fn count(pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM http_stats");
        sql.push_str(&Self::condition_sql(params)?);
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;
        Ok(count)
    }
    async fn get_by_id(pool: &Pool<MySql>, id: u64) -> Result<Option<Self>> {
        let stat = sqlx::query_as::<_, HttpStatSchema>(r#"SELECT * FROM http_stats WHERE id = ?"#)
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;
        Ok(stat.map(|schema| schema.into()))
    }
}
