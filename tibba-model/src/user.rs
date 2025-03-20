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
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{MySql, Pool};
use time::OffsetDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Disabled = 0,
    Enabled = 1,
}

#[derive(FromRow, Deserialize, Serialize)]
pub struct ModelUser {
    pub id: i64,
    pub status: i8,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub account: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub roles: Option<Json<Vec<String>>>,
    pub groups: Option<Json<Vec<String>>>,
    pub remark: Option<String>,
    pub email: Option<String>,
}

impl ModelUser {
    /// Create a new user with the given account and password
    pub async fn insert(pool: &Pool<MySql>, account: &str, password: &str) -> Result<i64> {
        // Get current time for created_at and updated_at
        let now = OffsetDateTime::now_utc();

        // Insert user and return the last insert ID
        let result = sqlx::query!(
            r#"
            INSERT INTO users (
                status, created_at, updated_at, account, password
            ) VALUES (
                ?, ?, ?, ?, ?
            )
            "#,
            UserStatus::Enabled as i8,
            now,
            now,
            account,
            password
        )
        .execute(pool)
        .await
        .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result.last_insert_id() as i64)
    }
    pub async fn get_by_account(pool: &Pool<MySql>, account: &str) -> Result<Option<Self>> {
        let result = sqlx::query_as::<_, Self>(r#"SELECT * FROM users WHERE account = ?"#)
            .bind(account)
            .fetch_optional(pool)
            .await
            .map_err(|e| Error::Sqlx { source: e })?;

        Ok(result)
    }
}
