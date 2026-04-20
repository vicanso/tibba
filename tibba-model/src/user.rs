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
    Error, JsonSnafu, Model, ModelListParams, Schema, SchemaAllowEdit, SchemaType, SchemaView,
    SqlxSnafu, Status, format_datetime, new_schema_options,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use std::collections::HashMap;
use substring::Substring;
use time::OffsetDateTime;
type Result<T> = std::result::Result<T, Error>;

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_SUPER_ADMIN: &str = "su";

#[derive(FromRow)]
struct UserSchema {
    id: i64,
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
    pub id: i64,
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
        Self {
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

pub struct UserModel {}

#[async_trait]
impl Model for UserModel {
    type Output = User;
    fn new() -> Self {
        Self {}
    }
    fn keyword(&self) -> String {
        "account".to_string()
    }
    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
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
                roles: vec![ROLE_SUPER_ADMIN.to_string()],
                ..Default::default()
            },
            ..Default::default()
        }
    }
    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, UserSchema>(
            r#"SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(result.map(|user| user.into()))
    }
    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE users SET deleted_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#
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
        data: serde_json::Value,
    ) -> Result<()> {
        let params: UserUpdateParams = serde_json::from_value(data).context(JsonSnafu)?;
        let _ = sqlx::query(
            r#"
            UPDATE users SET
                email = COALESCE($1, email),
                avatar = COALESCE($2, avatar),
                roles = COALESCE($3, roles),
                groups = COALESCE($4, groups),
                status = COALESCE($5, status)
            WHERE id = $6 AND deleted_at IS NULL
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(params.roles.map(Json))
        .bind(params.groups.map(Json))
        .bind(params.status)
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(())
    }
    fn filter_condition_sql(&self, filters: &HashMap<String, String>) -> Option<Vec<String>> {
        let mut conditions = vec![];
        if let Some(status) = filters.get("status") {
            conditions.push(format!("status = {status}"));
        }
        if let Some(role) = filters.get("role") {
            conditions.push(format!("roles @> '[\"{role}\"]'::jsonb"));
        }
        if let Some(group) = filters.get("group") {
            conditions.push(format!("groups @> '[\"{group}\"]'::jsonb"));
        }
        (!conditions.is_empty()).then_some(conditions)
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM users");
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
        let mut sql = String::from("SELECT * FROM users");

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

        let result = sqlx::query_as::<_, UserSchema>(&sql)
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;

        Ok(result.into_iter().map(|user| user.into()).collect())
    }
}

impl UserModel {
    pub async fn register(
        &self,
        pool: &Pool<Postgres>,
        account: &str,
        password: &str,
    ) -> Result<u64> {
        // Get current time for created_at and updated_at

        // Insert user and return the last insert ID
        let row: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO users (
                status, account, password
            ) VALUES (
                $1, $2, $3
            ) RETURNING id
            "#,
        )
        .bind(Status::Enabled as i8)
        .bind(account)
        .bind(password)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(row.0 as u64)
    }

    pub async fn get_by_account(
        &self,
        pool: &Pool<Postgres>,
        account: &str,
    ) -> Result<Option<User>> {
        let result = sqlx::query_as::<_, UserSchema>(
            r#"SELECT * FROM users WHERE account = $1 AND deleted_at IS NULL"#,
        )
        .bind(account)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(result.map(|user| user.into()))
    }

    pub async fn update_by_account(
        &self,
        pool: &Pool<Postgres>,
        account: &str,
        params: UserUpdateParams,
    ) -> Result<()> {
        let _ = sqlx::query(
            r#"
            UPDATE users SET
                email = COALESCE($1, email),
                avatar = COALESCE($2, avatar)
            WHERE account = $3 AND deleted_at IS NULL
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(account)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }
}
