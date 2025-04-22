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
    Error, ModelListParams, ROLE_ADMIN, ROLE_SUPER_ADMIN, Schema, SchemaAllowCreate,
    SchemaAllowEdit, SchemaOption, SchemaOptionValue, SchemaType, SchemaView, format_datetime,
};
use http::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{MySql, Pool};
use std::str::FromStr;
use substring::Substring;
use time::OffsetDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct FileSchema {
    id: u64,
    filename: String,
    file_size: i64,
    content_type: String,
    group: String,
    image_width: Option<i32>,
    image_height: Option<i32>,
    metadata: Option<Json<serde_json::Value>>,
    uploader: String,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Deserialize, Serialize)]
pub struct File {
    pub id: u64,
    pub filename: String,
    pub file_size: i64,
    pub content_type: String,
    pub group: String,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub metadata: Option<serde_json::Value>,
    pub uploader: String,
    pub created: String,
    pub modified: String,
}

impl From<FileSchema> for File {
    fn from(file: FileSchema) -> Self {
        File {
            id: file.id,
            filename: file.filename,
            file_size: file.file_size,
            content_type: file.content_type,
            group: file.group,
            image_width: file.image_width.map(|w| w as u32),
            image_height: file.image_height.map(|h| h as u32),
            metadata: file.metadata.map(|m| m.0),
            uploader: file.uploader,
            created: format_datetime(file.created),
            modified: format_datetime(file.modified),
        }
    }
}
impl File {
    pub fn get_metadata(&self) -> Option<HeaderMap> {
        let mut headers = HeaderMap::with_capacity(4);
        let Some(metadata) = &self.metadata else {
            return None;
        };
        let obj = metadata.as_object()?;
        for (key, value) in obj.iter() {
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
        Some(headers)
    }
}
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileInsertParams {
    pub group: String,
    pub filename: String,
    pub file_size: i64,
    pub content_type: String,
    pub uploader: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileUpdateParams {
    pub metadata: Option<serde_json::Value>,
}

impl From<serde_json::Value> for FileUpdateParams {
    fn from(value: serde_json::Value) -> Self {
        FileUpdateParams {
            metadata: value.get("metadata").cloned(),
        }
    }
}

impl File {
    pub fn schema_view() -> SchemaView {
        let group_options = vec![
            SchemaOption {
                label: "Tibba".to_string(),
                value: SchemaOptionValue::String("tibba".to_string()),
            },
            SchemaOption {
                label: "Web".to_string(),
                value: SchemaOptionValue::String("web".to_string()),
            },
        ];
        SchemaView {
            schemas: vec![
                Schema {
                    name: "id".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    hidden: true,
                    ..Default::default()
                },
                Schema {
                    name: "filename".to_string(),
                    category: SchemaType::String,
                    identity: true,
                    read_only: true,
                    required: true,
                    fixed: true,
                    ..Default::default()
                },
                Schema {
                    name: "file_size".to_string(),
                    category: SchemaType::Bytes,
                    read_only: true,
                    required: true,
                    sortable: true,
                    ..Default::default()
                },
                Schema {
                    name: "uploader".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    required: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "content_type".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "group".to_string(),
                    category: SchemaType::String,
                    options: Some(group_options.clone()),
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "image_width".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "image_height".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "metadata".to_string(),
                    category: SchemaType::Json,
                    ..Default::default()
                },
                Schema {
                    name: "created".to_string(),
                    category: SchemaType::Date,
                    read_only: true,
                    hidden: true,
                    ..Default::default()
                },
                Schema {
                    name: "modified".to_string(),
                    category: SchemaType::Date,
                    read_only: true,
                    sortable: true,
                    ..Default::default()
                },
            ],
            allow_edit: SchemaAllowEdit {
                owner: true,
                groups: vec![],
                roles: vec![ROLE_SUPER_ADMIN.to_string(), ROLE_ADMIN.to_string()],
            },
            allow_create: SchemaAllowCreate {
                roles: vec!["*".to_string()],
                ..Default::default()
            },
        }
    }

    fn condition_sql(params: &ModelListParams) -> Result<Option<String>> {
        let mut where_conditions = vec!["deleted_at IS NULL".to_string()];

        if let Some(keyword) = &params.keyword {
            where_conditions.push(format!("filename LIKE '%{}%'", keyword));
        }

        if let Some(filters) = params.parse_filters()? {
            if let Some(group) = filters.get("group") {
                where_conditions.push(format!("`group` = '{}'", group));
            }
            if let Some(uploader) = filters.get("uploader") {
                where_conditions.push(format!("uploader = '{}'", uploader));
            }
        }

        if !where_conditions.is_empty() {
            let sql = format!(" WHERE {}", where_conditions.join(" AND "));
            Ok(Some(sql))
        } else {
            Ok(None)
        }
    }

    pub async fn insert(pool: &Pool<MySql>, params: FileInsertParams) -> Result<u64> {
        let id = sqlx::query(
            r#"
            INSERT INTO files (
                `group`, filename, file_size, content_type,
                image_width, image_height, metadata, uploader
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(params.group)
        .bind(params.filename)
        .bind(params.file_size)
        .bind(params.content_type)
        .bind(params.width)
        .bind(params.height)
        .bind(params.metadata)
        .bind(params.uploader)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(id.last_insert_id())
    }

    pub async fn get_by_id(pool: &Pool<MySql>, id: u64) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, FileSchema>(
            r#"SELECT * FROM files WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|file| file.into()))
    }

    pub async fn get_by_name(pool: &Pool<MySql>, name: &str) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, FileSchema>(
            r#"SELECT * FROM files WHERE filename = ? AND deleted_at IS NULL"#,
        )
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|file| file.into()))
    }

    pub async fn delete_by_id(pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE files SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL"#
        )
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;
        Ok(())
    }

    pub async fn update_by_id(pool: &Pool<MySql>, id: u64, params: FileUpdateParams) -> Result<()> {
        let _ = sqlx::query(r#"UPDATE files SET metadata = ? WHERE id = ? AND deleted_at IS NULL"#)
            .bind(params.metadata)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;
        Ok(())
    }

    pub async fn count(pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM files");
        if let Some(condition) = Self::condition_sql(params)? {
            sql.push_str(&condition);
        }
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;
        Ok(count)
    }

    pub async fn list(pool: &Pool<MySql>, params: &ModelListParams) -> Result<Vec<File>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM files");
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

        let files = sqlx::query_as::<_, FileSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(files.into_iter().map(|file| file.into()).collect())
    }
}
