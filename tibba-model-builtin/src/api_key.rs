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

//! `api_keys` 表的 CRUD 接口（API Key / 个人访问令牌 PAT）。
//!
//! 安全约束：库中只存 `key_hash`（`sha256(明文)`），明文仅创建时返回一次。
//! - [`ApiKeyModel::create`] —— 新建 key（写入哈希 + 前缀 + 可选过期）
//! - [`ApiKeyModel::find_active_by_hash`] —— 鉴权热点：按哈希命中未撤销且未过期的 key
//! - [`ApiKeyModel::list_by_user`] —— 个人中心展示（不含哈希）
//! - [`ApiKeyModel::revoke`] —— 吊销（软删除，校验归属，返回 key_hash 供缓存失效）
//! - [`ApiKeyModel::touch_last_used`] —— 鉴权成功后更新 `last_used_at`

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::{Pool, Postgres};
use tibba_model::{Error, SqlxSnafu, format_datetime};
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct ApiKeySchema {
    id: i64,
    user_id: i64,
    name: String,
    key_prefix: String,
    last_used_at: Option<PrimitiveDateTime>,
    expires_at: Option<PrimitiveDateTime>,
    created: PrimitiveDateTime,
}

/// 单条 API Key 记录（对外展示，**不含** key_hash 与明文）。
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ApiKey {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub key_prefix: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub created: String,
}

impl From<ApiKeySchema> for ApiKey {
    fn from(s: ApiKeySchema) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            name: s.name,
            key_prefix: s.key_prefix,
            last_used_at: s.last_used_at.map(format_datetime),
            expires_at: s.expires_at.map(format_datetime),
            created: format_datetime(s.created),
        }
    }
}

/// 鉴权命中结果：仅暴露定位用户所需的最小字段。
///
/// 带 serde 是为了能进缓存（见 `tibba-router-user` 的 `lookup_api_key`）：
/// 鉴权在每个请求上都要跑，结果必须可序列化才谈得上缓存。字段只有 id 与
/// user_id，不含任何凭据material，落 Redis 不扩大泄漏面。
#[derive(FromRow, Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyAuth {
    pub id: i64,
    pub user_id: i64,
}

/// 新建 API Key 的入参。
#[derive(Debug, Clone)]
pub struct CreateApiKeyParams<'a> {
    pub user_id: i64,
    pub name: &'a str,
    pub key_prefix: &'a str,
    pub key_hash: &'a str,
    /// 过期天数；`None` 表示永不过期。
    pub expires_in_days: Option<i32>,
}

#[derive(Default)]
pub struct ApiKeyModel;

impl ApiKeyModel {
    pub fn new() -> Self {
        Self
    }

    /// 新建 key，返回新记录 id。过期时间由 `expires_in_days` 在库侧用 interval 计算，
    /// 避免 Rust 侧做时间运算（NULL → 永不过期）。
    pub async fn create(
        &self,
        pool: &Pool<Postgres>,
        params: CreateApiKeyParams<'_>,
    ) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO api_keys (user_id, name, key_prefix, key_hash, expires_at)
               VALUES ($1, $2, $3, $4,
                       CASE WHEN $5::int IS NULL THEN NULL
                            ELSE CURRENT_TIMESTAMP + make_interval(days => $5::int) END)
               RETURNING id"#,
        )
        .bind(params.user_id)
        .bind(params.name)
        .bind(params.key_prefix)
        .bind(params.key_hash)
        .bind(params.expires_in_days)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0)
    }

    /// 鉴权热点：按哈希命中未撤销且未过期的 key，返回归属用户。
    pub async fn find_active_by_hash(
        &self,
        pool: &Pool<Postgres>,
        key_hash: &str,
    ) -> Result<Option<ApiKeyAuth>> {
        let row: Option<ApiKeyAuth> = sqlx::query_as(
            r#"SELECT id, user_id FROM api_keys
               WHERE key_hash = $1 AND deleted_at IS NULL
                 AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
               LIMIT 1"#,
        )
        .bind(key_hash)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row)
    }

    /// 列出指定用户的所有生效 key（不含哈希），按创建时间倒序。
    pub async fn list_by_user(&self, pool: &Pool<Postgres>, user_id: i64) -> Result<Vec<ApiKey>> {
        let rows: Vec<ApiKeySchema> = sqlx::query_as(
            r#"SELECT id, user_id, name, key_prefix, last_used_at, expires_at, created
               FROM api_keys
               WHERE user_id = $1 AND deleted_at IS NULL
               ORDER BY created DESC"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(ApiKey::from).collect())
    }

    /// 吊销（软删除）指定 key，校验归属（只能删自己的）。
    ///
    /// 返回被吊销记录的 `key_hash`；`None` 表示没有匹配记录（不存在、已吊销，
    /// 或属于别人）。
    ///
    /// 之所以返回 hash 而不是行数：鉴权侧按 `key_hash` 缓存查询结果，吊销后必须
    /// 立刻把那条缓存删掉。只拿到行数的话，调用方还得再查一次库才知道要失效哪个
    /// key，而这里用 `RETURNING` 顺手就带回来了。
    pub async fn revoke(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        id: i64,
    ) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"UPDATE api_keys SET deleted_at = NOW()
               WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
               RETURNING key_hash"#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.map(|(hash,)| hash))
    }

    /// 鉴权成功后更新最近使用时间。尽力而为，调用方可忽略错误。
    pub async fn touch_last_used(&self, pool: &Pool<Postgres>, id: i64) -> Result<()> {
        sqlx::query(r#"UPDATE api_keys SET last_used_at = NOW() WHERE id = $1"#)
            .bind(id)
            .execute(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(())
    }
}
