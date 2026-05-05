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
    Error, JsonSnafu, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaType,
    SchemaView, SqlxSnafu, format_datetime,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use tibba_error::Error as AppError;
use tibba_model::Model;
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct TokenAccountSchema {
    id: i64,
    user_id: i64,
    balance: i64,
    total_recharged: i64,
    total_consumed: i64,
    status: i16,
    remark: String,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenAccount {
    pub id: i64,
    pub user_id: i64,
    pub balance: i64,
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub status: i16,
    pub remark: String,
    pub created: String,
    pub modified: String,
}

impl From<TokenAccountSchema> for TokenAccount {
    fn from(s: TokenAccountSchema) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            balance: s.balance,
            total_recharged: s.total_recharged,
            total_consumed: s.total_consumed,
            status: s.status,
            remark: s.remark,
            created: format_datetime(s.created),
            modified: format_datetime(s.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenAccountInsertParams {
    pub user_id: i64,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenAccountUpdateParams {
    pub status: Option<i16>,
    pub remark: Option<String>,
}

#[derive(Default)]
pub struct TokenAccountModel {}

impl TokenAccountModel {
    /// 按用户 ID 查询账户。
    pub async fn get_by_user_id(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
    ) -> Result<Option<TokenAccount>> {
        let result = sqlx::query_as::<_, TokenAccountSchema>(
            r#"SELECT * FROM token_accounts WHERE user_id = $1 AND deleted_at IS NULL"#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.map(Into::into))
    }

    /// 若账户不存在则自动创建，返回当前账户信息。
    /// 通常在用户注册后调用。
    pub async fn get_or_create(&self, pool: &Pool<Postgres>, user_id: i64) -> Result<TokenAccount> {
        sqlx::query(
            r#"INSERT INTO token_accounts (user_id) VALUES ($1) ON CONFLICT (user_id) WHERE deleted_at IS NULL DO NOTHING"#,
        )
        .bind(user_id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;

        self.get_by_user_id(pool, user_id)
            .await?
            .ok_or(Error::NotFound)
    }

    /// 原子性增加余额（充值）。
    /// 同步更新 total_recharged 汇总字段。
    /// 返回更新后的余额。
    pub async fn add_balance(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        amount: i64,
    ) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            UPDATE token_accounts
               SET balance          = balance + $1,
                   total_recharged  = total_recharged + $1
             WHERE user_id = $2 AND deleted_at IS NULL
             RETURNING balance"#,
        )
        .bind(amount)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0)
    }

    /// 原子性扣减余额（消费）。
    /// 余额不足时返回 HTTP 402，不会产生负余额。
    /// 返回扣减后的余额。
    pub async fn deduct_balance(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        amount: i64,
    ) -> std::result::Result<i64, AppError> {
        let result = sqlx::query_as::<_, (i64,)>(
            r#"
            UPDATE token_accounts
               SET balance         = balance - $1,
                   total_consumed  = total_consumed + $1
             WHERE user_id = $2
               AND balance >= $1
               AND status = 1
               AND deleted_at IS NULL
             RETURNING balance"#,
        )
        .bind(amount)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            AppError::new(e)
                .with_category("token")
                .with_sub_category("sqlx")
                .with_exception(true)
        })?;

        match result {
            Some(row) => Ok(row.0),
            None => Err(AppError::new("insufficient balance")
                .with_category("token")
                .with_sub_category("insufficient_balance")
                .with_status(402)),
        }
    }
}

impl Model for TokenAccountModel {
    type Output = TokenAccount;
    fn new() -> Self {
        Self::default()
    }

    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema::new_user_search("user_id"),
                Schema {
                    name: "balance".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "total_recharged".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "total_consumed".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema::new_status(),
                Schema::new_remark(),
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

    async fn insert(&self, pool: &Pool<Postgres>, data: serde_json::Value) -> Result<u64> {
        let params: TokenAccountInsertParams = serde_json::from_value(data).context(JsonSnafu)?;
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO token_accounts (user_id, remark) VALUES ($1, $2) RETURNING id"#,
        )
        .bind(params.user_id)
        .bind(params.remark.unwrap_or_default())
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0 as u64)
    }

    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, TokenAccountSchema>(
            r#"SELECT * FROM token_accounts WHERE id = $1 AND deleted_at IS NULL"#,
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
        let params: TokenAccountUpdateParams = serde_json::from_value(data).context(JsonSnafu)?;
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("UPDATE token_accounts SET modified = NOW()");
        if let Some(status) = params.status {
            qb.push(", status = ").push_bind(status);
        }
        if let Some(remark) = params.remark {
            qb.push(", remark = ").push_bind(remark);
        }
        qb.push(" WHERE id = ").push_bind(id as i64);
        qb.push(" AND deleted_at IS NULL");
        qb.build().execute(pool).await.context(SqlxSnafu)?;
        Ok(())
    }

    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE token_accounts SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM token_accounts");
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
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM token_accounts");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb);
        let rows = qb
            .build_query_as::<TokenAccountSchema>()
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    fn push_filter_conditions<'args>(
        &self,
        qb: &mut QueryBuilder<'args, Postgres>,
        filters: &HashMap<String, String>,
    ) -> Result<()> {
        if let Some(status) = filters.get("status") {
            if let Ok(s) = status.parse::<i16>() {
                qb.push(" AND status = ").push_bind(s);
            }
        }
        Ok(())
    }
}
