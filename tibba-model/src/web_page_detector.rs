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
    SchemaType, SchemaView, SqlxSnafu, format_datetime, new_schema_options,
};
use super::{REGION_ALIYUN, REGION_ANY, REGION_GZ, REGION_TX};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use substring::Substring;
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct WebPageDetectorSchema {
    id: i64,
    status: i16,
    name: String,
    interval: i16,
    url: String,
    width: i32,
    height: i32,
    user_agent: String,
    accept_language: String,
    platform: String,
    wait_for_element: String,
    device_scale_factor: f64,
    timeout: i32,
    capture_screenshot: bool,
    capture_element: String,
    remark: String,
    regions: Json<Vec<String>>,
    created_by: i64,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct WebPageDetector {
    pub id: i64,
    pub status: i16,
    pub name: String,
    pub interval: i16,
    pub url: String,
    pub width: i32,
    pub height: i32,
    pub user_agent: String,
    pub accept_language: String,
    pub platform: String,
    pub wait_for_element: String,
    pub device_scale_factor: f64,
    pub timeout: i32,
    pub capture_screenshot: bool,
    pub capture_element: String,
    pub remark: String,
    pub regions: Vec<String>,
    pub created_by: i64,
    pub created: String,
    pub modified: String,
}

impl From<WebPageDetectorSchema> for WebPageDetector {
    fn from(schema: WebPageDetectorSchema) -> Self {
        Self {
            id: schema.id,
            status: schema.status,
            name: schema.name,
            interval: schema.interval,
            url: schema.url,
            width: schema.width,
            height: schema.height,
            user_agent: schema.user_agent,
            accept_language: schema.accept_language,
            platform: schema.platform,
            wait_for_element: schema.wait_for_element,
            device_scale_factor: schema.device_scale_factor,
            timeout: schema.timeout,
            capture_screenshot: schema.capture_screenshot,
            capture_element: schema.capture_element,
            remark: schema.remark,
            regions: schema.regions.0,
            created_by: schema.created_by,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct WebPageDetectorInsertParams {
    pub name: String,
    pub interval: u16,
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub user_agent: Option<String>,
    pub accept_language: Option<String>,
    pub platform: Option<String>,
    pub wait_for_element: Option<String>,
    pub device_scale_factor: Option<f64>,
    pub timeout: Option<u32>,
    pub capture_screenshot: Option<bool>,
    pub capture_element: Option<String>,
    pub regions: Vec<String>,
    pub remark: String,
    pub created_by: u64,
}

pub struct WebPageDetectorModel {}

impl Model for WebPageDetectorModel {
    type Output = WebPageDetector;
    fn new() -> Self {
        Self {}
    }
    fn keyword(&self) -> String {
        "name".to_string()
    }
    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
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
                    category: SchemaType::String,
                    required: true,
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
                    name: "width".to_string(),
                    category: SchemaType::Number,
                    ..Default::default()
                },
                Schema {
                    name: "height".to_string(),
                    category: SchemaType::Number,
                    ..Default::default()
                },
                Schema {
                    name: "user_agent".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "accept_language".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "platform".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "wait_for_element".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "device_scale_factor".to_string(),
                    category: SchemaType::Number,
                    ..Default::default()
                },
                Schema {
                    name: "timeout".to_string(),
                    category: SchemaType::Number,
                    ..Default::default()
                },
                Schema {
                    name: "capture_screenshot".to_string(),
                    category: SchemaType::Boolean,
                    ..Default::default()
                },
                Schema {
                    name: "capture_element".to_string(),
                    category: SchemaType::String,
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
    fn condition_sql(&self, params: &ModelListParams) -> Result<String> {
        let mut where_conditions = vec!["deleted_at IS NULL".to_string()];

        if let Some(keyword) = &params.keyword {
            where_conditions.push(format!("{} LIKE '%{}%'", self.keyword(), keyword));
        }
        Ok(format!(" WHERE {}", where_conditions.join(" AND ")))
    }
    async fn insert(&self, pool: &Pool<Postgres>, params: serde_json::Value) -> Result<u64> {
        let params: WebPageDetectorInsertParams =
            serde_json::from_value(params).context(JsonSnafu)?;
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO web_page_detectors (name, interval, url, width, height, user_agent, accept_language, platform, wait_for_element, device_scale_factor, timeout, capture_screenshot, capture_element, remark, regions, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) RETURNING id"#,
        )
        .bind(params.name)
        .bind(params.interval as i16)
        .bind(params.url)
        .bind(params.width as i32)
        .bind(params.height as i32)
        .bind(params.user_agent.unwrap_or_default())
        .bind(params.accept_language.unwrap_or_default())
        .bind(params.platform.unwrap_or_default())
        .bind(params.wait_for_element.unwrap_or_default())
        .bind(params.device_scale_factor.unwrap_or_default())
        .bind(params.timeout.unwrap_or_default() as i32)
        .bind(params.capture_screenshot.unwrap_or_default())
        .bind(params.capture_element.unwrap_or_default())
        .bind(params.remark)
        .bind(Json(params.regions))
        .bind(params.created_by as i64)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(row.0 as u64)
    }
    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM web_page_detectors");
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
        let mut sql = String::from("SELECT * FROM web_page_detectors");
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

        let detectors = sqlx::query_as::<_, WebPageDetectorSchema>(&sql)
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;

        Ok(detectors.into_iter().map(|schema| schema.into()).collect())
    }
}

impl WebPageDetectorModel {
    pub async fn list_enabled_by_region(
        &self,
        pool: &Pool<Postgres>,
        region: Option<String>,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<WebPageDetector>> {
        let region = region.unwrap_or(REGION_ANY.to_string());
        let detectors = sqlx::query_as::<_, WebPageDetectorSchema>(
            r#"SELECT * FROM web_page_detectors WHERE deleted_at IS NULL AND status = 1 AND (jsonb_array_length(regions) = 0 OR regions @> $1::jsonb OR regions @> $2::jsonb) ORDER BY id ASC LIMIT $3 OFFSET $4"#,
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
