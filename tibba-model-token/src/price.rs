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
    SchemaView, SqlxSnafu, Status, format_datetime,
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
struct TokenPriceSchema {
    id: i64,
    service: String,
    model: String,
    input_price: i64,
    output_price: i64,
    fixed_price: i64,
    unit_size: i32,
    status: i16,
    remark: String,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenPrice {
    pub id: i64,
    pub service: String,
    pub model: String,
    /// 每 unit_size 个输入 token 扣除的积分数
    pub input_price: i64,
    /// 每 unit_size 个输出 token 扣除的积分数
    pub output_price: i64,
    /// 每次调用固定扣除积分数
    pub fixed_price: i64,
    /// 计费基数，默认 1000（per 1K tokens）
    pub unit_size: i32,
    pub status: i16,
    pub remark: String,
    pub created: String,
    pub modified: String,
}

impl From<TokenPriceSchema> for TokenPrice {
    fn from(s: TokenPriceSchema) -> Self {
        Self {
            id: s.id,
            service: s.service,
            model: s.model,
            input_price: s.input_price,
            output_price: s.output_price,
            fixed_price: s.fixed_price,
            unit_size: s.unit_size,
            status: s.status,
            remark: s.remark,
            created: format_datetime(s.created),
            modified: format_datetime(s.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenPriceInsertParams {
    pub service: String,
    pub model: Option<String>,
    pub input_price: i64,
    pub output_price: i64,
    pub fixed_price: Option<i64>,
    pub unit_size: Option<i32>,
    pub status: Option<i16>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenPriceUpdateParams {
    pub input_price: Option<i64>,
    pub output_price: Option<i64>,
    pub fixed_price: Option<i64>,
    pub unit_size: Option<i32>,
    pub status: Option<i16>,
    pub remark: Option<String>,
}

#[derive(Default)]
pub struct TokenPriceModel {}

impl TokenPriceModel {
    /// 按服务类型和模型名查询定价配置。
    /// 先精确匹配 (service, model)，找不到时退回匹配 (service, "")。
    pub async fn get_by_service_model(
        &self,
        pool: &Pool<Postgres>,
        service: &str,
        model: &str,
    ) -> Result<Option<TokenPrice>> {
        // 精确匹配
        let result = sqlx::query_as::<_, TokenPriceSchema>(
            r#"SELECT * FROM token_prices
               WHERE service = $1 AND model = $2 AND status = 1 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(service)
        .bind(model)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        if result.is_some() {
            return Ok(result.map(Into::into));
        }

        // 回退：匹配该服务的默认定价（model = ''）
        if !model.is_empty() {
            let fallback = sqlx::query_as::<_, TokenPriceSchema>(
                r#"SELECT * FROM token_prices
                   WHERE service = $1 AND model = '' AND status = 1 AND deleted_at IS NULL
                   LIMIT 1"#,
            )
            .bind(service)
            .fetch_optional(pool)
            .await
            .context(SqlxSnafu)?;
            return Ok(fallback.map(Into::into));
        }

        Ok(None)
    }

    /// 根据定价配置和 token 用量计算本次消耗积分。
    /// 使用整数向上取整，避免浮点误差。
    pub fn calculate_cost(price: &TokenPrice, input_tokens: i32, output_tokens: i32) -> i64 {
        let unit = price.unit_size.max(1) as i64;
        // 向上取整：(n * p + unit - 1) / unit
        let input_cost = (input_tokens as i64 * price.input_price + unit - 1) / unit;
        let output_cost = (output_tokens as i64 * price.output_price + unit - 1) / unit;
        price.fixed_price + input_cost + output_cost
    }
}

impl Model for TokenPriceModel {
    type Output = TokenPrice;
    fn new() -> Self {
        Self::default()
    }

    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "service".to_string(),
                    category: SchemaType::String,
                    required: true,
                    fixed: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "model".to_string(),
                    category: SchemaType::String,
                    fixed: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "input_price".to_string(),
                    category: SchemaType::Number,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "output_price".to_string(),
                    category: SchemaType::Number,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "fixed_price".to_string(),
                    category: SchemaType::Number,
                    ..Default::default()
                },
                Schema {
                    name: "unit_size".to_string(),
                    category: SchemaType::Number,
                    default_value: Some(serde_json::json!(1000)),
                    ..Default::default()
                },
                Schema::new_status(),
                Schema::new_remark(),
                Schema::new_created(),
                Schema::new_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                roles: vec!["su".to_string()],
                ..Default::default()
            },
            allow_create: SchemaAllowCreate {
                roles: vec!["su".to_string()],
                ..Default::default()
            },
        }
    }

    async fn insert(&self, pool: &Pool<Postgres>, data: serde_json::Value) -> Result<u64> {
        let p: TokenPriceInsertParams = serde_json::from_value(data).context(JsonSnafu)?;
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO token_prices
               (service, model, input_price, output_price, fixed_price, unit_size, status, remark)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               RETURNING id"#,
        )
        .bind(&p.service)
        .bind(p.model.unwrap_or_default())
        .bind(p.input_price)
        .bind(p.output_price)
        .bind(p.fixed_price.unwrap_or(0))
        .bind(p.unit_size.unwrap_or(1000))
        .bind(p.status.unwrap_or(Status::Enabled as i16))
        .bind(p.remark.unwrap_or_default())
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0 as u64)
    }

    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, TokenPriceSchema>(
            r#"SELECT * FROM token_prices WHERE id = $1 AND deleted_at IS NULL"#,
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
        let p: TokenPriceUpdateParams = serde_json::from_value(data).context(JsonSnafu)?;
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("UPDATE token_prices SET modified = NOW()");
        if let Some(v) = p.input_price {
            qb.push(", input_price = ").push_bind(v);
        }
        if let Some(v) = p.output_price {
            qb.push(", output_price = ").push_bind(v);
        }
        if let Some(v) = p.fixed_price {
            qb.push(", fixed_price = ").push_bind(v);
        }
        if let Some(v) = p.unit_size {
            qb.push(", unit_size = ").push_bind(v);
        }
        if let Some(v) = p.status {
            qb.push(", status = ").push_bind(v);
        }
        if let Some(v) = p.remark {
            qb.push(", remark = ").push_bind(v);
        }
        qb.push(" WHERE id = ")
            .push_bind(id as i64)
            .push(" AND deleted_at IS NULL");
        qb.build().execute(pool).await.context(SqlxSnafu)?;
        Ok(())
    }

    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE token_prices SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT COUNT(*) FROM token_prices");
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
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM token_prices");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb);
        let rows = qb
            .build_query_as::<TokenPriceSchema>()
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
        if let Some(service) = filters.get("service") {
            qb.push(" AND service = ").push_bind(service.clone());
        }
        if let Some(status) = filters.get("status") {
            if let Ok(v) = status.parse::<i16>() {
                qb.push(" AND status = ").push_bind(v);
            }
        }
        Ok(())
    }
}
