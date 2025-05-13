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
    Error, Model, ModelListParams, Schema, SchemaAllowEdit, SchemaType, SchemaView, Status,
    format_datetime, new_schema_options,
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

pub static ROLE_ADMIN: &str = "admin";
pub static ROLE_SUPER_ADMIN: &str = "su";

#[derive(FromRow)]
struct UserSchema {
    id: u64,
    status: i8,
    created: OffsetDateTime,
    modified: OffsetDateTime,
    account: String,
    password: String,
    roles: Option<Json<Vec<String>>>,
    groups: Option<Json<Vec<String>>>,
    remark: Option<String>,
    email: Option<String>,
    avatar: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct User {
    pub id: u64,
    pub status: i8,
    pub created: String,
    pub modified: String,
    pub account: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub roles: Option<Vec<String>>,
    pub groups: Option<Vec<String>>,
    pub remark: Option<String>,
    pub email: Option<String>,
    pub avatar: Option<String>,
}

impl From<UserSchema> for User {
    fn from(user: UserSchema) -> Self {
        User {
            id: user.id,
            status: user.status,
            created: format_datetime(user.created),
            modified: format_datetime(user.modified),
            account: user.account,
            password: user.password,
            roles: user.roles.map(|roles| roles.0),
            groups: user.groups.map(|groups| groups.0),
            remark: user.remark,
            email: user.email,
            avatar: user.avatar,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserUpdateParams {
    pub email: Option<String>,
    pub avatar: Option<String>,
    pub roles: Option<Vec<String>>,
    pub groups: Option<Vec<String>>,
    pub status: Option<i8>,
}

#[async_trait]
impl Model for User {
    type Output = User;
    fn keyword() -> String {
        "account".to_string()
    }
    fn schema_view() -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "account".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    required: true,
                    identity: true,
                    ..Default::default()
                },
                Schema::new_status(),
                Schema {
                    name: "roles".to_string(),
                    category: SchemaType::Strings,
                    options: Some(new_schema_options(&[ROLE_ADMIN, ROLE_SUPER_ADMIN])),
                    ..Default::default()
                },
                Schema {
                    name: "groups".to_string(),
                    category: SchemaType::Strings,
                    options: Some(new_schema_options(&["it", "marketing"])),
                    ..Default::default()
                },
                Schema::new_created(),
                Schema::new_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                owner: true,
                groups: vec![],
                roles: vec![ROLE_SUPER_ADMIN.to_string()],
            },
            ..Default::default()
        }
    }
    async fn get_by_id(pool: &Pool<MySql>, id: u64) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, UserSchema>(
            r#"SELECT * FROM users WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|user| user.into()))
    }
    async fn delete_by_id(pool: &Pool<MySql>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE users SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL"#
        )
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;
        Ok(())
    }
    async fn update_by_id(pool: &Pool<MySql>, id: u64, data: serde_json::Value) -> Result<()> {
        let params: UserUpdateParams =
            serde_json::from_value(data).map_err(|e| Error::Json { source: e })?;
        let _ = sqlx::query(
            r#"
            UPDATE users SET 
                email = COALESCE(?, email),
                avatar = COALESCE(?, avatar),
                roles = COALESCE(?, roles),
                `groups` = COALESCE(?, `groups`),
                status = COALESCE(?, status)
            WHERE id = ? AND deleted_at IS NULL
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(params.roles.map(Json))
        .bind(params.groups.map(Json))
        .bind(params.status)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }
    fn filter_condition_sql(filters: &HashMap<String, String>) -> Option<Vec<String>> {
        let mut where_conditions = vec![];
        if let Some(status) = filters.get("status") {
            where_conditions.push(format!("status = {}", status));
        }
        if let Some(role) = filters.get("role") {
            where_conditions.push(format!("JSON_CONTAINS(roles, JSON_ARRAY('{}'))", role));
        }
        if let Some(group) = filters.get("group") {
            where_conditions.push(format!("JSON_CONTAINS(groups, JSON_ARRAY('{}'))", group));
        }
        Some(where_conditions)
    }

    async fn count(pool: &Pool<MySql>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM users");
        sql.push_str(&Self::condition_sql(params)?);
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;
        Ok(count)
    }

    async fn list(pool: &Pool<MySql>, params: &ModelListParams) -> Result<Vec<Self>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM users");

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

        let result = sqlx::query_as::<_, UserSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.into_iter().map(|user| user.into()).collect())
    }
}

impl User {
    pub async fn register(pool: &Pool<MySql>, account: &str, password: &str) -> Result<u64> {
        // Get current time for created_at and updated_at

        // Insert user and return the last insert ID
        let result = sqlx::query(
            r#"
            INSERT INTO users (
                status, account, password
            ) VALUES (
                ?, ?, ?
            )
            "#,
        )
        .bind(Status::Enabled as i8)
        .bind(account)
        .bind(password)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id())
    }

    pub async fn get_by_account(pool: &Pool<MySql>, account: &str) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, UserSchema>(
            r#"SELECT * FROM users WHERE account = ? AND deleted_at IS NULL"#,
        )
        .bind(account)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|user| user.into()))
    }

    pub async fn update_by_account(
        pool: &Pool<MySql>,
        account: &str,
        params: UserUpdateParams,
    ) -> Result<()> {
        let _ = sqlx::query(
            r#"
            UPDATE users SET 
                email = COALESCE(?, email),
                avatar = COALESCE(?, avatar)
            WHERE account = ? AND deleted_at IS NULL
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(account)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;
        Ok(())
    }
}
