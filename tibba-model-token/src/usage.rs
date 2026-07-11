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
struct TokenUsageSchema {
    id: i64,
    user_id: i64,
    service: String,
    amount: i64,
    model: String,
    input_tokens: i32,
    output_tokens: i32,
    api_path: String,
    duration_ms: i32,
    biz_id: String,
    remark: String,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenUsage {
    pub id: i64,
    pub user_id: i64,
    pub service: String,
    pub amount: i64,
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub api_path: String,
    pub duration_ms: i32,
    pub biz_id: String,
    pub remark: String,
    pub created: String,
    pub modified: String,
}

impl From<TokenUsageSchema> for TokenUsage {
    fn from(s: TokenUsageSchema) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            service: s.service,
            amount: s.amount,
            model: s.model,
            input_tokens: s.input_tokens,
            output_tokens: s.output_tokens,
            api_path: s.api_path,
            duration_ms: s.duration_ms,
            biz_id: s.biz_id,
            remark: s.remark,
            created: format_datetime(s.created),
            modified: format_datetime(s.modified),
        }
    }
}

/// 记录一次消耗的参数，由调用方（如 tibba-llm）填写后传入。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenUsageInsertParams {
    pub user_id: i64,
    pub service: String,
    pub amount: i64,
    /// LLM 场景填模型名，其他场景留空
    pub model: Option<String>,
    /// LLM 输入 token 数
    pub input_tokens: Option<i32>,
    /// LLM 输出 token 数
    pub output_tokens: Option<i32>,
    /// 通用 API 场景的路径
    pub api_path: Option<String>,
    /// 调用耗时（毫秒）
    pub duration_ms: Option<i32>,
    /// 关联业务 ID（请求 ID、任务 ID 等）
    pub biz_id: Option<String>,
    pub remark: Option<String>,
}

/// 按用户/服务维度的消耗汇总。
#[derive(Debug, Clone, Serialize)]
pub struct TokenUsageSummary {
    pub user_id: i64,
    pub service: String,
    pub model: String,
    pub total_amount: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub call_count: i64,
}

#[derive(Default)]
pub struct TokenUsageModel {}

impl TokenUsageModel {
    /// 按用户 ID 查询消耗记录（分页，倒序）。
    pub async fn list_by_user(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        page: u64,
        limit: u64,
    ) -> Result<Vec<TokenUsage>> {
        let limit = limit.min(200);
        let offset = (page.max(1) - 1) * limit;
        let rows = sqlx::query_as::<_, TokenUsageSchema>(
            r#"SELECT * FROM token_usages WHERE user_id = $1 AND deleted_at IS NULL ORDER BY id DESC LIMIT $2 OFFSET $3"#,
        )
        .bind(user_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// 按服务+模型维度聚合消耗汇总（用于统计分析）。
    pub async fn summary_by_service(
        &self,
        pool: &Pool<Postgres>,
        user_id: Option<i64>,
    ) -> Result<Vec<TokenUsageSummary>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"SELECT user_id, service, model,
                      SUM(amount) AS total_amount,
                      SUM(input_tokens) AS total_input_tokens,
                      SUM(output_tokens) AS total_output_tokens,
                      COUNT(*) AS call_count
               FROM token_usages
               WHERE deleted_at IS NULL"#,
        );
        if let Some(uid) = user_id {
            qb.push(" AND user_id = ").push_bind(uid);
        }
        qb.push(" GROUP BY user_id, service, model ORDER BY total_amount DESC");

        #[derive(FromRow)]
        struct SummaryRow {
            user_id: i64,
            service: String,
            model: String,
            total_amount: i64,
            total_input_tokens: i64,
            total_output_tokens: i64,
            call_count: i64,
        }

        let rows = qb
            .build_query_as::<SummaryRow>()
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;

        Ok(rows
            .into_iter()
            .map(|r| TokenUsageSummary {
                user_id: r.user_id,
                service: r.service,
                model: r.model,
                total_amount: r.total_amount,
                total_input_tokens: r.total_input_tokens,
                total_output_tokens: r.total_output_tokens,
                call_count: r.call_count,
            })
            .collect())
    }
}

impl Model for TokenUsageModel {
    type Output = TokenUsage;
    fn new() -> Self {
        Self::default()
    }

    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "user_id".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "service".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "amount".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "model".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "input_tokens".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "output_tokens".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "api_path".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "duration_ms".to_string(),
                    category: SchemaType::Number,
                    read_only: true,
                    ..Default::default()
                },
                Schema {
                    name: "biz_id".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    ..Default::default()
                },
                Schema::new_readonly_remark(),
                Schema::new_created(),
                Schema::new_filterable_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                disabled: true,
                ..Default::default()
            },
            allow_create: SchemaAllowCreate {
                disabled: true,
                ..Default::default()
            },
        }
    }

    async fn insert(&self, pool: &Pool<Postgres>, data: serde_json::Value) -> Result<u64> {
        let p: TokenUsageInsertParams = serde_json::from_value(data).context(JsonSnafu)?;
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO token_usages
               (user_id, service, amount, model, input_tokens, output_tokens, api_path, duration_ms, biz_id, remark)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               RETURNING id"#,
        )
        .bind(p.user_id)
        .bind(&p.service)
        .bind(p.amount)
        .bind(p.model.unwrap_or_default())
        .bind(p.input_tokens.unwrap_or(0))
        .bind(p.output_tokens.unwrap_or(0))
        .bind(p.api_path.unwrap_or_default())
        .bind(p.duration_ms.unwrap_or(0))
        .bind(p.biz_id.unwrap_or_default())
        .bind(p.remark.unwrap_or_default())
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0 as u64)
    }

    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, TokenUsageSchema>(
            r#"SELECT * FROM token_usages WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.map(Into::into))
    }

    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        sqlx::query(
            r#"UPDATE token_usages SET deleted_at = NOW(), modified = NOW() WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT COUNT(*) FROM token_usages");
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
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM token_usages");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb, self.orderable_columns());
        let rows = qb
            .build_query_as::<TokenUsageSchema>()
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
        if let Some(service) = filters.get("service") {
            qb.push(" AND service = ").push_bind(service.clone());
        }
        if let Some(model) = filters.get("model") {
            qb.push(" AND model = ").push_bind(model.clone());
        }
        Ok(())
    }
}
