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
    SchemaView, SqlxSnafu, Status, format_datetime, new_schema_options,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use tibba_model::Model;
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

/// 后端协议：openai（默认）或 anthropic
pub const LLM_PROVIDER_OPENAI: &str = "openai";
pub const LLM_PROVIDER_ANTHROPIC: &str = "anthropic";

#[derive(FromRow)]
struct TokenLlmSchema {
    id: i64,
    name: String,
    url: String,
    model: String,
    api_key: String,
    provider: String,
    status: i16,
    remark: String,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenLlm {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub model: String,
    pub api_key: String,
    /// 后端协议：openai 或 anthropic，留空时按 openai 处理
    pub provider: String,
    pub status: i16,
    pub remark: String,
    pub created: String,
    pub modified: String,
}

impl From<TokenLlmSchema> for TokenLlm {
    fn from(s: TokenLlmSchema) -> Self {
        Self {
            id: s.id,
            name: s.name,
            url: s.url,
            model: s.model,
            api_key: s.api_key,
            provider: s.provider,
            status: s.status,
            remark: s.remark,
            created: format_datetime(s.created),
            modified: format_datetime(s.modified),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenLlmInsertParams {
    pub name: String,
    pub url: String,
    pub model: String,
    pub api_key: String,
    pub provider: Option<String>,
    pub status: Option<i16>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenLlmUpdateParams {
    pub url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub status: Option<i16>,
    pub remark: Option<String>,
}

#[derive(Default)]
pub struct TokenLlmModel {}

impl TokenLlmModel {
    /// 按 name 查询启用状态的 LLM 配置；未命中时回退到 name = "default"。
    pub async fn get_by_name(&self, pool: &Pool<Postgres>, name: &str) -> Result<Option<TokenLlm>> {
        let result = sqlx::query_as::<_, TokenLlmSchema>(
            r#"SELECT * FROM token_llms
               WHERE name = $1 AND status = 1 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(name)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        if result.is_some() {
            return Ok(result.map(Into::into));
        }

        // 回退：匹配 name = "default"（避免 name 已是 default 时重复查询）
        if name != "default" {
            let fallback = sqlx::query_as::<_, TokenLlmSchema>(
                r#"SELECT * FROM token_llms
                   WHERE name = 'default' AND status = 1 AND deleted_at IS NULL
                   LIMIT 1"#,
            )
            .fetch_optional(pool)
            .await
            .context(SqlxSnafu)?;
            return Ok(fallback.map(Into::into));
        }

        Ok(None)
    }
}

impl Model for TokenLlmModel {
    type Output = TokenLlm;
    fn new() -> Self {
        Self::default()
    }

    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "name".to_string(),
                    category: SchemaType::String,
                    required: true,
                    fixed: true,
                    filterable: true,
                    ..Default::default()
                },
                Schema {
                    name: "url".to_string(),
                    category: SchemaType::String,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "model".to_string(),
                    category: SchemaType::String,
                    required: true,
                    filterable: true,
                    options: Some(new_schema_options(&["mimo-v2.5-pro"])),
                    ..Default::default()
                },
                Schema {
                    name: "api_key".to_string(),
                    category: SchemaType::String,
                    required: true,
                    ..Default::default()
                },
                Schema {
                    name: "provider".to_string(),
                    category: SchemaType::String,
                    filterable: true,
                    options: Some(new_schema_options(&[
                        LLM_PROVIDER_OPENAI,
                        LLM_PROVIDER_ANTHROPIC,
                    ])),
                    default_value: Some(serde_json::json!(LLM_PROVIDER_OPENAI)),
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
        let p: TokenLlmInsertParams = serde_json::from_value(data).context(JsonSnafu)?;
        let provider = p
            .provider
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| LLM_PROVIDER_OPENAI.to_string());
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO token_llms
               (name, url, model, api_key, provider, status, remark)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING id"#,
        )
        .bind(&p.name)
        .bind(&p.url)
        .bind(&p.model)
        .bind(&p.api_key)
        .bind(provider)
        .bind(p.status.unwrap_or(Status::Enabled as i16))
        .bind(p.remark.unwrap_or_default())
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0 as u64)
    }

    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let result = sqlx::query_as::<_, TokenLlmSchema>(
            r#"SELECT * FROM token_llms WHERE id = $1 AND deleted_at IS NULL"#,
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
        let p: TokenLlmUpdateParams = serde_json::from_value(data).context(JsonSnafu)?;
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("UPDATE token_llms SET modified = NOW()");
        if let Some(v) = p.url {
            qb.push(", url = ").push_bind(v);
        }
        if let Some(v) = p.model {
            qb.push(", model = ").push_bind(v);
        }
        if let Some(v) = p.api_key {
            qb.push(", api_key = ").push_bind(v);
        }
        if let Some(v) = p.provider {
            qb.push(", provider = ").push_bind(v);
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
            r#"UPDATE token_llms SET deleted_at = NOW(), modified = NOW() WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id as i64)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT COUNT(*) FROM token_llms");
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
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM token_llms");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb);
        let rows = qb
            .build_query_as::<TokenLlmSchema>()
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
        if let Some(name) = filters.get("name") {
            qb.push(" AND name = ").push_bind(name.clone());
        }
        if let Some(model) = filters.get("model") {
            qb.push(" AND model = ").push_bind(model.clone());
        }
        if let Some(provider) = filters.get("provider") {
            qb.push(" AND provider = ").push_bind(provider.clone());
        }
        if let Some(status) = filters.get("status") {
            if let Ok(v) = status.parse::<i16>() {
                qb.push(" AND status = ").push_bind(v);
            }
        }
        Ok(())
    }
}
