// Copyright 2026 Tree xie.
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
    Error, JsonSnafu, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaType,
    SchemaView, SqlxSnafu, format_datetime,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use tibba_model::Model;
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct TokenKeySchema {
    id: i64,
    user_id: i64,
    token: String,
    name: String,
    status: i16,
    expired_at: Option<PrimitiveDateTime>,
    created_by: i64,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenKey {
    pub id: i64,
    pub user_id: i64,
    pub token: String,
    pub name: String,
    pub status: i16,
    pub expired_at: Option<String>,
    pub created_by: i64,
    pub created: String,
    pub modified: String,
}

impl From<TokenKeySchema> for TokenKey {
    fn from(s: TokenKeySchema) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            token: s.token,
            name: s.name,
            status: s.status,
            expired_at: s.expired_at.map(format_datetime),
            created_by: s.created_by,
            created: format_datetime(s.created),
            modified: format_datetime(s.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenKeyInsertParams {
    pub user_id: i64,
    pub name: String,
    pub created_by: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenKeyUpdateParams {
    pub name: Option<String>,
    pub status: Option<i16>,
    pub expired_at: Option<String>,
}

#[derive(Default)]
pub struct TokenKeyModel {}

impl TokenKeyModel {
    /// 根据 token 字符串查询绑定的 user_id，供鉴权中间件使用。
    /// 仅返回未删除、已启用、且未过期的记录。
    pub async fn get_user_id_by_token(
        &self,
        pool: &Pool<Postgres>,
        token: &str,
    ) -> Result<Option<i64>> {
        let result = sqlx::query_as::<_, (i64,)>(
            r#"SELECT user_id FROM token_keys
                WHERE token = $1
                  AND status = 1
                  AND deleted_at IS NULL
                  AND (expired_at IS NULL OR expired_at > NOW())"#,
        )
        .bind(token)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.map(|r| r.0))
    }
}

impl Model for TokenKeyModel {
    type Output = TokenKey;
    fn new() -> Self {
        Self::default()
    }

    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema::new_user_search("user_id"),
                Schema {
                    name: "token".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    auto_create: true,
                    popover: true,
                    ..Default::default()
                },
                Schema::new_name(),
                Schema::new_status(),
                Schema {
                    name: "expired_at".to_string(),
                    category: SchemaType::Date,
                    ..Default::default()
                },
                Schema {
                    name: "created_by".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    hidden: true,
                    auto_create: true,
                    ..Default::default()
                },
                Schema::new_created(),
                Schema::new_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                roles: vec!["su".to_string(), "admin".to_string()],
                ..Default::default()
            },
            allow_create: SchemaAllowCreate {
                roles: vec!["su".to_string(), "admin".to_string()],
                ..Default::default()
            },
        }
    }

    async fn insert(&self, pool: &Pool<Postgres>, mut data: serde_json::Value) -> Result<u64> {
        // user_id 支持前端以字符串形式传入
        if let Some(obj) = data.as_object_mut() {
            if let Some(id_str) = obj.get("user_id").and_then(|v| v.as_str()) {
                if let Ok(id) = id_str.parse::<i64>() {
                    obj.insert("user_id".to_string(), id.into());
                }
            }
        }
        let p: TokenKeyInsertParams = serde_json::from_value(data).context(JsonSnafu)?;
        let token = uuid::Uuid::new_v4().to_string();
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO token_keys (user_id, name, token, created_by)
               VALUES ($1, $2, $3, $4) RETURNING id"#,
        )
        .bind(p.user_id)
        .bind(&p.name)
        .bind(&token)
        .bind(p.created_by.unwrap_or(0))
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0 as u64)
    }

    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, TokenKeySchema>(
            r#"SELECT * FROM token_keys WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.map(Into::into))
    }

    async fn update_by_id(
        &self,
        pool: &Pool<Postgres>,
        id: u64,
        data: serde_json::Value,
    ) -> Result<()> {
        let p: TokenKeyUpdateParams = serde_json::from_value(data).context(JsonSnafu)?;
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("UPDATE token_keys SET modified = NOW()");
        if let Some(name) = p.name {
            qb.push(", name = ").push_bind(name);
        }
        if let Some(status) = p.status {
            qb.push(", status = ").push_bind(status);
        }
        if let Some(expired_at) = p.expired_at {
            if expired_at.is_empty() {
                qb.push(", expired_at = NULL");
            } else {
                // 之前把 datetime 解析失败包成 `NotSupported`——语义错位，会让客户端
                // 看到 "not supported function: invalid expired_at" 的奇怪报错。
                // `tibba-model` 早就有 `Error::InvalidDatetime { value }` 专门表达
                // 这种格式问题，且 `parse_primitive_datetime` 本身就返回它，直接 `?` 透传
                let dt = tibba_model::parse_primitive_datetime(&expired_at)?;
                qb.push(", expired_at = ").push_bind(dt);
            }
        }
        qb.push(" WHERE id = ").push_bind(id as i64);
        qb.push(" AND deleted_at IS NULL");
        qb.build().execute(pool).await.context(SqlxSnafu)?;
        Ok(())
    }

    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE token_keys SET deleted_at = NOW(), modified = NOW() WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT COUNT(*) FROM token_keys");
        self.push_conditions(&mut qb, params)?;
        let row: (i64,) = qb
            .build_query_as()
            .fetch_one(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(row.0)
    }

    async fn list(
        &self,
        pool: &Pool<Postgres>,
        params: &ModelListParams,
    ) -> Result<Vec<Self::Output>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM token_keys");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb, self.orderable_columns());
        let rows = qb
            .build_query_as::<TokenKeySchema>()
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    fn push_filter_conditions(
        &self,
        qb: &mut QueryBuilder<Postgres>,
        filters: &HashMap<String, String>,
    ) -> Result<()> {
        if let Some(user_id) = filters.get("user_id") {
            if let Ok(v) = user_id.parse::<i64>() {
                qb.push(" AND user_id = ").push_bind(v);
            }
        }
        Ok(())
    }
}
