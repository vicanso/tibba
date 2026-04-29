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
    Error, JsonSnafu, Model, ModelListParams, Schema, SchemaAllowEdit, SchemaOption,
    SchemaOptionValue, SchemaType, SchemaView, SqlxSnafu, Status, format_datetime,
    new_schema_options,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use time::PrimitiveDateTime;
type Result<T> = std::result::Result<T, Error>;

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_SUPER_ADMIN: &str = "su";

#[derive(FromRow)]
struct UserSchema {
    id: i64,
    status: i16,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
    account: String,
    password: String,
    nickname: Option<String>,
    phone: Option<String>,
    roles: Option<Json<Vec<String>>>,
    groups: Option<Json<Vec<String>>>,
    remark: Option<String>,
    email: Option<String>,
    avatar: Option<String>,
    last_login_at: Option<PrimitiveDateTime>,
}

#[derive(Deserialize, Serialize)]
pub struct User {
    pub id: i64,
    pub status: i16,
    pub created: String,
    pub modified: String,
    pub account: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub nickname: Option<String>,
    pub phone: Option<String>,
    pub roles: Option<Vec<String>>,
    pub groups: Option<Vec<String>>,
    pub remark: Option<String>,
    pub email: Option<String>,
    pub avatar: Option<String>,
    pub last_login_at: Option<String>,
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
            nickname: user.nickname,
            phone: user.phone,
            roles: user.roles.map(|roles| roles.0),
            groups: user.groups.map(|groups| groups.0),
            remark: user.remark,
            email: user.email,
            avatar: user.avatar,
            last_login_at: user.last_login_at.map(format_datetime),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserUpdateParams {
    pub nickname: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub avatar: Option<String>,
    pub roles: Option<Vec<String>>,
    pub groups: Option<Vec<String>>,
    pub status: Option<i16>,
}

pub struct UserModel {}

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
                    name: "nickname".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "phone".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
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
                Schema {
                    name: "last_login_at".to_string(),
                    category: SchemaType::Date,
                    read_only: true,
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
                status = COALESCE($5, status),
                nickname = COALESCE($6, nickname),
                phone = COALESCE($7, phone)
            WHERE id = $8 AND deleted_at IS NULL
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(params.roles.map(Json))
        .bind(params.groups.map(Json))
        .bind(params.status)
        .bind(params.nickname.as_deref())
        .bind(params.phone.as_deref())
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(())
    }
    fn push_filter_conditions<'args>(
        &self,
        qb: &mut QueryBuilder<'args, Postgres>,
        filters: &HashMap<String, String>,
    ) -> Result<()> {
        if let Some(status) = filters.get("status").and_then(|s| s.parse::<i16>().ok()) {
            qb.push(" AND status = ");
            qb.push_bind(status);
        }
        if let Some(role) = filters.get("role") {
            qb.push(" AND roles @> ");
            qb.push_bind(Json(vec![role.clone()]));
            qb.push("::jsonb");
        }
        if let Some(group) = filters.get("group") {
            qb.push(" AND groups @> ");
            qb.push_bind(Json(vec![group.clone()]));
            qb.push("::jsonb");
        }
        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb = QueryBuilder::new("SELECT COUNT(*) FROM users");
        self.push_conditions(&mut qb, params)?;
        let count = qb
            .build_query_scalar::<i64>()
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
        let mut qb = QueryBuilder::new("SELECT * FROM users");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb);
        let result = qb
            .build_query_as::<UserSchema>()
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(result.into_iter().map(|u| u.into()).collect())
    }
    async fn search_options(
        &self,
        pool: &Pool<Postgres>,
        keyword: Option<String>,
    ) -> Result<Vec<SchemaOption>> {
        let params = ModelListParams {
            keyword,
            limit: 20,
            page: 1,
            ..Default::default()
        };
        let users = self.list(pool, &params).await?;
        Ok(users
            .into_iter()
            .map(|u| SchemaOption {
                label: u.account,
                value: SchemaOptionValue::String(u.id.to_string()),
            })
            .collect())
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
        .bind(Status::Enabled as i16)
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
                avatar = COALESCE($2, avatar),
                nickname = COALESCE($3, nickname),
                phone = COALESCE($4, phone)
            WHERE account = $5 AND deleted_at IS NULL
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(params.nickname.as_deref())
        .bind(params.phone.as_deref())
        .bind(account)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    /// 登录成功后更新 last_login_at 为当前时间。
    pub async fn update_last_login_at(&self, pool: &Pool<Postgres>, account: &str) -> Result<()> {
        sqlx::query(
            r#"UPDATE users SET last_login_at = CURRENT_TIMESTAMP WHERE account = $1 AND deleted_at IS NULL"#,
        )
        .bind(account)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }
}
