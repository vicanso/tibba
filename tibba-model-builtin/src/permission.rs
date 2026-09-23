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

/// 权限点的 CRUD 接口。`code` 在未删除行中唯一（部分唯一索引），`upsert` 按此幂等。
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

    /// 注册新的权限点；已存在活跃行（同 code）时按部分唯一索引触发 ON CONFLICT，
    /// 覆盖更新描述字段。ON CONFLICT 谓词须与 `uk_permissions_code (code) WHERE
    /// deleted_at IS NULL` 一致，否则无法匹配部分索引（42P10）。
    pub async fn upsert(
        &self,
        pool: &Pool<Postgres>,
        code: &str,
        description: &str,
    ) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO permissions (code, description)
               VALUES ($1, $2)
               ON CONFLICT (code) WHERE deleted_at IS NULL DO UPDATE
                 SET description = EXCLUDED.description,
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

    /// 软删除指定权限点，**并在同一事务里撤销所有角色对它的授予**。返回删除的权限点行数。
    ///
    /// 此前只删 `permissions` 表，文档写着「调用方若需要级联应自行处理」。但授权
    /// 判定（`RolePermissionModel::list_permissions_for_roles`）只查映射表，于是
    /// 「删除权限点」对已授予它的角色**完全不生效**——删除一个危险权限的唯一效果
    /// 是它从管理界面上消失，持有它的人照用不误。
    ///
    /// 放进事务：避免权限点已删、映射还在的中间态被并发的登录读到。
    pub async fn soft_delete_by_code(&self, pool: &Pool<Postgres>, code: &str) -> Result<u64> {
        let mut tx = pool.begin().await.context(SqlxSnafu)?;
        let result = sqlx::query(
            r#"UPDATE permissions
               SET deleted_at = NOW(), modified = NOW()
               WHERE code = $1 AND deleted_at IS NULL"#,
        )
        .bind(code)
        .execute(&mut *tx)
        .await
        .context(SqlxSnafu)?;
        sqlx::query(
            r#"UPDATE role_permissions
               SET deleted_at = NOW()
               WHERE permission_code = $1 AND deleted_at IS NULL"#,
        )
        .bind(code)
        .execute(&mut *tx)
        .await
        .context(SqlxSnafu)?;
        tx.commit().await.context(SqlxSnafu)?;
        Ok(result.rows_affected())
    }
}
