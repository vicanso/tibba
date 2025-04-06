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

use super::{Error, ModelListParams, format_datetime};
use schemars::{JsonSchema, Schema, schema_for};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{MySql, Pool};
use substring::Substring;
use time::OffsetDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct FileSchema {
    id: i64,
    filename: String,
    file_size: i64,
    content_type: String,
    group: String,
    image_width: Option<i32>,
    image_height: Option<i32>,
    metadata: Option<Json<serde_json::Value>>,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct File {
    pub id: u64,
    pub filename: String,
    pub file_size: i64,
    pub content_type: String,
    pub group: String,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub metadata: Option<serde_json::Value>,
    pub created: String,
    pub modified: String,
}

impl From<FileSchema> for File {
    fn from(file: FileSchema) -> Self {
        File {
            id: file.id as u64,
            filename: file.filename,
            file_size: file.file_size,
            content_type: file.content_type,
            group: file.group,
            image_width: file.image_width.map(|w| w as u32),
            image_height: file.image_height.map(|h| h as u32),
            metadata: file.metadata.map(|m| m.0),
            created: format_datetime(file.created),
            modified: format_datetime(file.modified),
        }
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

impl File {
    pub fn schema() -> Schema {
        schema_for!(File)
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

    pub async fn list(pool: &Pool<MySql>, params: ModelListParams) -> Result<Vec<File>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM files");

        let mut where_conditions = vec![];

        if let Some(keyword) = params.keyword {
            where_conditions.push(format!("filename LIKE '%{}%'", keyword));
        }

        if !where_conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_conditions.join(" AND "));
        }

        if let Some(order_by) = params.order_by {
            let (order_by, direction) = if order_by.starts_with("-") {
                (order_by.substring(1, order_by.len()).to_string(), "DESC")
            } else {
                (order_by, "ASC")
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
