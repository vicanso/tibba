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
    Error, Model, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaOption,
    SchemaOptionValue, SchemaType, SchemaView, format_datetime,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::{MySql, Pool};
use std::collections::HashMap;
use time::OffsetDateTime;

type Result<T> = std::result::Result<T, Error>;

// 在相应的模型文件中定义
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorGroupRole {
    Owner = 1,
    Admin = 2,
    Member = 3,
    Viewer = 4,
}

impl DetectorGroupRole {
    // pub fn from_i8(value: i8) -> Option<Self> {
    //     match value {
    //         1 => Some(Self::Owner),
    //         2 => Some(Self::Admin),
    //         3 => Some(Self::Member),
    //         4 => Some(Self::Viewer),
    //         _ => None,
    //     }
    // }

    pub fn to_i8(self) -> i8 {
        self as i8
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Viewer => "viewer",
        }
    }
}

impl std::fmt::Display for DetectorGroupRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(FromRow)]
struct DetectorGroupUserSchema {
    id: u64,
    user_id: u64,
    group_id: u64,
    role: i8,
    status: i8,
    effective_start_time: OffsetDateTime,
    effective_end_time: OffsetDateTime,
    invited_by: u64,
    remark: String,
    created: OffsetDateTime,
    modified: OffsetDateTime,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DetectorGroupUser {
    pub id: u64,
    pub user_id: u64,
    pub group_id: u64,
    pub role: i8,
    pub status: i8,
    pub effective_start_time: String,
    pub effective_end_time: String,
    pub invited_by: u64,
    pub remark: String,
    pub created: String,
    pub modified: String,
}

impl From<DetectorGroupUserSchema> for DetectorGroupUser {
    fn from(schema: DetectorGroupUserSchema) -> Self {
        Self {
            id: schema.id,
            user_id: schema.user_id,
            group_id: schema.group_id,
            role: schema.role,
            status: schema.status,
            effective_start_time: format_datetime(schema.effective_start_time),
            effective_end_time: format_datetime(schema.effective_end_time),
            invited_by: schema.invited_by,
            remark: schema.remark,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DetectorGroupUserInsertParams {
    pub user_id: u64,
    pub group_id: u64,
    pub role: i8,
    pub status: i8,
    pub effective_start_time: String,
    pub effective_end_time: String,
    pub invited_by: u64,
    pub remark: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DetectorGroupUserUpdateParams {
    pub role: Option<i8>,
    pub status: Option<i8>,
    pub effective_start_time: Option<String>,
    pub effective_end_time: Option<String>,
    pub invited_by: Option<u64>,
    pub remark: Option<String>,
}

pub struct DetectorGroupUserModel {}

#[async_trait]
impl Model for DetectorGroupUserModel {
    type Output = DetectorGroupUser;
    fn new() -> Self {
        Self {}
    }
    async fn schema_view(&self, _pool: &Pool<MySql>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "user_id".to_string(),
                    category: SchemaType::Number,
                    ..Default::default()
                },
                Schema {
                    name: "group_id".to_string(),
                    category: SchemaType::Number,
                    ..Default::default()
                },
                Schema {
                    name: "role".to_string(),
                    category: SchemaType::Number,
                    options: Some(vec![
                        SchemaOption {
                            label: DetectorGroupRole::Owner.to_string(),
                            value: SchemaOptionValue::Integer(
                                DetectorGroupRole::Owner.to_i8() as i64
                            ),
                        },
                        SchemaOption {
                            label: DetectorGroupRole::Admin.to_string(),
                            value: SchemaOptionValue::Integer(
                                DetectorGroupRole::Admin.to_i8() as i64
                            ),
                        },
                        SchemaOption {
                            label: DetectorGroupRole::Member.to_string(),
                            value: SchemaOptionValue::Integer(
                                DetectorGroupRole::Member.to_i8() as i64
                            ),
                        },
                        SchemaOption {
                            label: DetectorGroupRole::Viewer.to_string(),
                            value: SchemaOptionValue::Integer(
                                DetectorGroupRole::Viewer.to_i8() as i64
                            ),
                        },
                    ]),
                    ..Default::default()
                },
                Schema::new_status(),
                Schema {
                    name: "effective_start_time".to_string(),
                    category: SchemaType::Date,
                    ..Default::default()
                },
                Schema {
                    name: "effective_end_time".to_string(),
                    category: SchemaType::Date,
                    ..Default::default()
                },
                Schema::new_remark(),
                Schema::new_created(),
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

        if let Some(group_id) = filters.get("group_id") {
            conditions.push(format!("group_id = '{group_id}'"));
        }

        (!conditions.is_empty()).then_some(conditions)
    }

    async fn insert(&self, pool: &Pool<MySql>, params: serde_json::Value) -> Result<u64> {
        let params: DetectorGroupUserInsertParams =
            serde_json::from_value(params).map_err(|e| Error::Json { source: e })?;
        let result = sqlx::query(
            r#"INSERT INTO detector_group_users (user_id, group_id, role, status, effective_start_time, effective_end_time, invited_by, remark) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(params.user_id)
        .bind(params.group_id)
        .bind(params.role)
        .bind(params.status)
        .bind(params.effective_start_time)
        .bind(params.effective_end_time)
        .bind(params.invited_by)
        .bind(params.remark)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id())
    }

    async fn get_by_id(&self, pool: &Pool<MySql>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, DetectorGroupUserSchema>(
            r#"SELECT * FROM detector_group_users WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|schema| schema.into()))
    }

    async fn delete_by_id(&self, pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(r#"UPDATE detector_group_users SET deleted_at = NOW() WHERE id = ?"#)
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
        let params: DetectorGroupUserUpdateParams =
            serde_json::from_value(params).map_err(|e| Error::Json { source: e })?;

        let _ = sqlx::query(
            r#"UPDATE detector_group_users SET role = COALESCE(?, role), status = COALESCE(?, status), effective_start_time = COALESCE(?, effective_start_time), effective_end_time = COALESCE(?, effective_end_time), invited_by = COALESCE(?, invited_by), remark = COALESCE(?, remark) WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(params.role)
        .bind(params.status)
        .bind(params.effective_start_time)
        .bind(params.effective_end_time)
        .bind(params.invited_by)
        .bind(params.remark)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }

    async fn count(&self, pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM detector_group_users");
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
        let mut sql = String::from("SELECT * FROM detector_group_users");
        sql.push_str(&self.condition_sql(params)?);
        let offset = (params.page - 1) * limit;
        sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));

        let users = sqlx::query_as::<_, DetectorGroupUserSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(users.into_iter().map(|schema| schema.into()).collect())
    }
}
