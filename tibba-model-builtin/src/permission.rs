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

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::{Pool, Postgres};
use tibba_model::{Error, SqlxSnafu, format_datetime};
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

#[derive(FromRow)]
struct PermissionSchema {
    id: i64,
    code: String,
    description: String,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
}

/// 单条权限点记录，对外暴露给 admin 接口列表。
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Permission {
    pub id: i64,
    pub code: String,
    pub description: String,
    pub created: String,
    pub modified: String,
}

impl From<PermissionSchema> for Permission {
    fn from(schema: PermissionSchema) -> Self {
        Self {
            id: schema.id,
            code: schema.code,
            description: schema.description,
            created: format_datetime(schema.created),
            modified: format_datetime(schema.modified),
        }
    }
}

/// 权限点的 CRUD 接口。`code` 字段唯一约束，重复插入返回 SQL 错误（由调用方决定如何处理）。
#[derive(Default)]
pub struct PermissionModel;

impl PermissionModel {
    pub fn new() -> Self {
        Self
    }

    /// 列出所有未软删除的权限点，按 code 升序。
    pub async fn list_all(&self, pool: &Pool<Postgres>) -> Result<Vec<Permission>> {
        let rows: Vec<PermissionSchema> = sqlx::query_as(
            r#"SELECT id, code, description, created, modified
               FROM permissions
               WHERE deleted_at IS NULL
               ORDER BY code ASC"#,
        )
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(Permission::from).collect())
    }

    /// 按 code 查找单条权限点；不存在或已软删除返回 None。
    pub async fn get_by_code(
        &self,
        pool: &Pool<Postgres>,
        code: &str,
    ) -> Result<Option<Permission>> {
        let row: Option<PermissionSchema> = sqlx::query_as(
            r#"SELECT id, code, description, created, modified
               FROM permissions
               WHERE code = $1 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(code)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.map(Permission::from))
    }

    /// 注册新的权限点；已存在（含软删除态）时按 code 唯一约束触发 ON CONFLICT，
    /// 描述字段会被覆盖更新，软删除态会被恢复。
    pub async fn upsert(
        &self,
        pool: &Pool<Postgres>,
        code: &str,
        description: &str,
    ) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO permissions (code, description)
               VALUES ($1, $2)
               ON CONFLICT (code) DO UPDATE
                 SET description = EXCLUDED.description,
                     deleted_at = NULL,
                     modified = NOW()
               RETURNING id"#,
        )
        .bind(code)
        .bind(description)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0)
    }

    /// 软删除指定权限点。注意：role_permissions 中引用此 code 的记录不会自动清理，
    /// 调用方若需要级联应自行处理。
    pub async fn soft_delete_by_code(&self, pool: &Pool<Postgres>, code: &str) -> Result<u64> {
        let result = sqlx::query(
            r#"UPDATE permissions
               SET deleted_at = NOW(), modified = NOW()
               WHERE code = $1 AND deleted_at IS NULL"#,
        )
        .bind(code)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.rows_affected())
    }
}
