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
    Error, JsonSnafu, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaOption,
    SchemaOptionValue, SchemaType, SchemaView, SqlxSnafu, format_datetime,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use time::PrimitiveDateTime;
use tibba_model::Model;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct TokenRechargeSchema {
    id: i64,
    user_id: i64,
    amount: i64,
    source: i16,
    order_id: String,
    expired_at: Option<PrimitiveDateTime>,
    remark: String,
    created_by: i64,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenRecharge {
    pub id: i64,
    pub user_id: i64,
    pub amount: i64,
    pub source: i16,
    pub order_id: String,
    pub expired_at: Option<String>,
    pub remark: String,
    pub created_by: i64,
    pub created: String,
    pub modified: String,
}

impl From<TokenRechargeSchema> for TokenRecharge {
    fn from(s: TokenRechargeSchema) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            amount: s.amount,
            source: s.source,
            order_id: s.order_id,
            expired_at: s.expired_at.map(format_datetime),
            remark: s.remark,
            created_by: s.created_by,
            created: format_datetime(s.created),
            modified: format_datetime(s.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenRechargeInsertParams {
    pub user_id: i64,
    pub amount: i64,
    pub source: i16,
    pub order_id: Option<String>,
    pub expired_at: Option<String>,
    pub remark: Option<String>,
    pub created_by: Option<i64>,
}

#[derive(Default)]
pub struct TokenRechargeModel {}

impl TokenRechargeModel {
    /// 按用户 ID 查询充值记录列表（分页）。
    pub async fn list_by_user(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        page: u64,
        limit: u64,
    ) -> Result<Vec<TokenRecharge>> {
        let limit = limit.min(200);
        let offset = (page.max(1) - 1) * limit;
        let rows = sqlx::query_as::<_, TokenRechargeSchema>(
            r#"SELECT * FROM token_recharges WHERE user_id = $1 AND deleted_at IS NULL ORDER BY id DESC LIMIT $2 OFFSET $3"#,
        )
        .bind(user_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// 按订单号查询（用于支付回调去重）。
    pub async fn get_by_order_id(
        &self,
        pool: &Pool<Postgres>,
        order_id: &str,
    ) -> Result<Option<TokenRecharge>> {
        let result = sqlx::query_as::<_, TokenRechargeSchema>(
            r#"SELECT * FROM token_recharges WHERE order_id = $1 AND deleted_at IS NULL LIMIT 1"#,
        )
        .bind(order_id)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.map(Into::into))
    }
}

impl Model for TokenRechargeModel {
    type Output = TokenRecharge;
    fn new() -> Self {
        Self::default()
    }

    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
        let source_options = vec![
            SchemaOption {
                label: "购买".to_string(),
                value: SchemaOptionValue::Integer(1),
            },
            SchemaOption {
                label: "赠送".to_string(),
                value: SchemaOptionValue::Integer(2),
            },
            SchemaOption {
                label: "退款".to_string(),
                value: SchemaOptionValue::Integer(3),
            },
            SchemaOption {
                label: "管理员调整".to_string(),
                value: SchemaOptionValue::Integer(4),
            },
        ];
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "user_id".to_string(),
                    category: SchemaType::Number,
                    required: true,
                    read_only: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "amount".to_string(),
                    category: SchemaType::Number,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "source".to_string(),
                    category: SchemaType::Number,
                    required: true,
                    options: Some(source_options),
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "order_id".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "expired_at".to_string(),
                    category: SchemaType::Date,
                    ..Default::default()
                },
                Schema::new_remark(),
                Schema {
                    name: "created_by".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema::new_created(),
                Schema::new_filterable_modified(),
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
        let p: TokenRechargeInsertParams = serde_json::from_value(data).context(JsonSnafu)?;
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO token_recharges (user_id, amount, source, order_id, remark, created_by)
               VALUES ($1, $2, $3, $4, $5, $6) RETURNING id"#,
        )
        .bind(p.user_id)
        .bind(p.amount)
        .bind(p.source)
        .bind(p.order_id.unwrap_or_default())
        .bind(p.remark.unwrap_or_default())
        .bind(p.created_by.unwrap_or(0))
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0 as u64)
    }

    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, TokenRechargeSchema>(
            r#"SELECT * FROM token_recharges WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.map(Into::into))
    }

    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE token_recharges SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM token_recharges");
        self.push_conditions(&mut qb, params)?;
        let row: (i64,) = qb.build_query_as().fetch_one(pool).await.context(SqlxSnafu)?;
        Ok(row.0)
    }

    async fn list(
        &self,
        pool: &Pool<Postgres>,
        params: &ModelListParams,
    ) -> Result<Vec<Self::Output>> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT * FROM token_recharges");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb);
        let rows = qb
            .build_query_as::<TokenRechargeSchema>()
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
        if let Some(user_id) = filters.get("user_id") {
            if let Ok(v) = user_id.parse::<i64>() {
                qb.push(" AND user_id = ").push_bind(v);
            }
        }
        if let Some(source) = filters.get("source") {
            if let Ok(v) = source.parse::<i16>() {
                qb.push(" AND source = ").push_bind(v);
            }
        }
        Ok(())
    }
}
