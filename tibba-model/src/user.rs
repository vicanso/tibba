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

use super::Error;
use schemars::{JsonSchema, Schema, schema_for};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{MySql, Pool};
use substring::Substring;
use time::{OffsetDateTime, UtcOffset};

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Disabled = 0,
    Enabled = 1,
}

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

#[derive(Deserialize, Serialize, JsonSchema)]
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
        let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        User {
            id: user.id,
            status: user.status,
            created: user.created.to_offset(offset).to_string(),
            modified: user.modified.to_offset(offset).to_string(),
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserListParams {
    pub page: u64,
    pub limit: u64,
    pub order_by: Option<String>,
}

impl User {
    pub fn schema() -> Schema {
        schema_for!(User)
    }
    /// Create a new user with the given account and password
    pub async fn insert(pool: &Pool<MySql>, account: &str, password: &str) -> Result<u64> {
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
        .bind(UserStatus::Enabled as i8)
        .bind(account)
        .bind(password)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id())
    }
    pub async fn get_by_account(pool: &Pool<MySql>, account: &str) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, UserSchema>(r#"SELECT * FROM users WHERE account = ?"#)
            .bind(account)
            .fetch_optional(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.map(|user| user.into()))
    }
    pub async fn update(pool: &Pool<MySql>, account: &str, params: UserUpdateParams) -> Result<()> {
        let _ = sqlx::query(
            r#"
            UPDATE users SET 
                email = COALESCE(?, email),
                avatar = COALESCE(?, avatar),
                roles = COALESCE(?, roles),
                `groups` = COALESCE(?, `groups`)
            WHERE account = ?
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(params.roles.map(Json))
        .bind(params.groups.map(Json))
        .bind(account)
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(())
    }
    pub async fn list(pool: &Pool<MySql>, params: UserListParams) -> Result<Vec<Self>> {
        let limit = params.limit.min(200);
        let mut sql = String::from("SELECT * FROM users");

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

        let result = sqlx::query_as::<_, UserSchema>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.into_iter().map(|user| user.into()).collect())
    }
}
