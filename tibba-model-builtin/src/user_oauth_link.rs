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

//! `user_oauth_links` 表的 CRUD 接口。
//!
//! 核心查询：
//! - [`UserOauthLinkModel::find_by_provider_uid`] —— OAuth callback 命中已绑定账号的关键路径
//! - [`UserOauthLinkModel::create`] —— 新建关联（自动合并 / 新注册都会用到）
//! - [`UserOauthLinkModel::list_by_user`] —— 个人中心展示「已绑定的第三方」
//! - [`UserOauthLinkModel::unlink`] —— 用户解绑（软删除）

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::{Pool, Postgres};
use tibba_model::{Error, SqlxSnafu, format_datetime};
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct UserOauthLinkSchema {
    id: i64,
    user_id: i64,
    provider: String,
    provider_user_id: String,
    provider_login: String,
    provider_email: String,
    created: PrimitiveDateTime,
}

/// 单条第三方身份关联记录。
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct UserOauthLink {
    pub id: i64,
    pub user_id: i64,
    pub provider: String,
    pub provider_user_id: String,
    pub provider_login: String,
    pub provider_email: String,
    pub created: String,
}

impl From<UserOauthLinkSchema> for UserOauthLink {
    fn from(s: UserOauthLinkSchema) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            provider: s.provider,
            provider_user_id: s.provider_user_id,
            provider_login: s.provider_login,
            provider_email: s.provider_email,
            created: format_datetime(s.created),
        }
    }
}

/// 新建关联时的入参。`provider_login` 和 `provider_email` 仅做快照，可为空串。
#[derive(Debug, Clone)]
pub struct CreateLinkParams<'a> {
    pub user_id: i64,
    pub provider: &'a str,
    pub provider_user_id: &'a str,
    pub provider_login: &'a str,
    pub provider_email: &'a str,
}

#[derive(Default)]
pub struct UserOauthLinkModel;

impl UserOauthLinkModel {
    pub fn new() -> Self {
        Self
    }

    /// OAuth callback 热点：按 (provider, provider_user_id) 找到已绑定的本地用户。
    /// 命中 → 直接登录该用户；未命中 → 走自动合并 / 新建逻辑。
    pub async fn find_by_provider_uid(
        &self,
        pool: &Pool<Postgres>,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<UserOauthLink>> {
        let row: Option<UserOauthLinkSchema> = sqlx::query_as(
            r#"SELECT id, user_id, provider, provider_user_id,
                      provider_login, provider_email, created
               FROM user_oauth_links
               WHERE provider = $1 AND provider_user_id = $2 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.map(UserOauthLink::from))
    }

    /// 建立新的关联。重复绑定会触发 UNIQUE 约束错误——调用方应先查 [`find_by_provider_uid`]。
    pub async fn create(
        &self,
        pool: &Pool<Postgres>,
        params: CreateLinkParams<'_>,
    ) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO user_oauth_links
                 (user_id, provider, provider_user_id, provider_login, provider_email)
               VALUES ($1, $2, $3, $4, $5)
               RETURNING id"#,
        )
        .bind(params.user_id)
        .bind(params.provider)
        .bind(params.provider_user_id)
        .bind(params.provider_login)
        .bind(params.provider_email)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0)
    }

    /// 列出指定用户的所有生效关联（个人中心展示）。
    pub async fn list_by_user(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
    ) -> Result<Vec<UserOauthLink>> {
        let rows: Vec<UserOauthLinkSchema> = sqlx::query_as(
            r#"SELECT id, user_id, provider, provider_user_id,
                      provider_login, provider_email, created
               FROM user_oauth_links
               WHERE user_id = $1 AND deleted_at IS NULL
               ORDER BY created ASC"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(UserOauthLink::from).collect())
    }

    /// 软删除指定用户的某 provider 关联（解绑）。返回受影响行数。
    pub async fn unlink(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        provider: &str,
    ) -> Result<u64> {
        let result = sqlx::query(
            r#"UPDATE user_oauth_links
               SET deleted_at = NOW()
               WHERE user_id = $1 AND provider = $2 AND deleted_at IS NULL"#,
        )
        .bind(user_id)
        .bind(provider)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.rows_affected())
    }
}
