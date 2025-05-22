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

use super::{Error, Model, ModelListParams, SchemaView, format_datetime};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{MySql, Pool};
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
    status: i32,
    tls: String,
    alpn: String,
    subject: String,
    issuer: String,
    cert_not_before: OffsetDateTime,
    cert_not_after: OffsetDateTime,
    cert_cipher: String,
    cert_domains: Json<Vec<String>>,
    body_size: i32,
    error: String,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Default, Deserialize, Serialize)]
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
    pub status: i32,
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
            status: schema.status,
            tls: schema.tls,
            alpn: schema.alpn,
            subject: schema.subject,
            issuer: schema.issuer,
            cert_not_before: format_datetime(schema.cert_not_before),
            cert_not_after: format_datetime(schema.cert_not_after),
            cert_cipher: schema.cert_cipher,
            cert_domains: schema.cert_domains.0,
            body_size: schema.body_size,
            error: schema.error,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]

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
    pub status: Option<i32>,
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
}

impl HttpStat {
    pub async fn add_stat(pool: &Pool<MySql>, params: HttpStatInsertParams) -> Result<u64> {
        let result = sqlx::query(
            r#"INSERT INTO http_stats (target_id, target_name, url, dns_lookup, quic_connect, tcp_connect, tls_handshake, server_processing, content_transfer, total, addr, status, tls, alpn, subject, issuer, cert_not_before, cert_not_after, cert_cipher, cert_domains, body_size, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
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
        .bind(params.status.unwrap_or(0))
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
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id())
    }
}

#[async_trait]
impl Model for HttpStat {
    type Output = Self;
    fn schema_view() -> SchemaView {
        SchemaView {
            ..Default::default()
        }
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
}
