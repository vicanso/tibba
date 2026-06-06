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
struct RolePermissionSchema {
    id: i64,
    role: String,
    permission_code: String,
    created: PrimitiveDateTime,
}

/// 单条角色-权限映射记录。
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct RolePermission {
    pub id: i64,
    pub role: String,
    pub permission_code: String,
    pub created: String,
}

impl From<RolePermissionSchema> for RolePermission {
    fn from(schema: RolePermissionSchema) -> Self {
        Self {
            id: schema.id,
            role: schema.role,
            permission_code: schema.permission_code,
            created: format_datetime(schema.created),
        }
    }
}

/// 角色-权限映射的 CRUD 接口。
///
/// 运行期的热点是 [`Self::list_permissions_for_roles`]：用户登录时根据 `users.roles`
/// 拉取并集，写进 `Session.permissions` 缓存到 Session 失效为止。
#[derive(Default)]
pub struct RolePermissionModel;

impl RolePermissionModel {
    pub fn new() -> Self {
        Self
    }

    /// 给指定角色授予权限码。已存在时静默忽略（幂等）。
    pub async fn grant(
        &self,
        pool: &Pool<Postgres>,
        role: &str,
        permission_code: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO role_permissions (role, permission_code)
               VALUES ($1, $2)
               ON CONFLICT (role, permission_code) DO UPDATE
                 SET deleted_at = NULL"#,
        )
        .bind(role)
        .bind(permission_code)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    /// 撤销指定角色的某个权限码（软删除）。返回受影响行数。
    pub async fn revoke(
        &self,
        pool: &Pool<Postgres>,
        role: &str,
        permission_code: &str,
    ) -> Result<u64> {
        let result = sqlx::query(
            r#"UPDATE role_permissions
               SET deleted_at = NOW()
               WHERE role = $1 AND permission_code = $2 AND deleted_at IS NULL"#,
        )
        .bind(role)
        .bind(permission_code)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.rows_affected())
    }

    /// 列出某个角色所有生效的权限码（仅返回 code 字符串，不返回元数据）。
    pub async fn list_codes_by_role(
        &self,
        pool: &Pool<Postgres>,
        role: &str,
    ) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"SELECT permission_code FROM role_permissions
               WHERE role = $1 AND deleted_at IS NULL"#,
        )
        .bind(role)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// 给定角色集合，返回它们的权限码并集（去重）。
    /// 登录场景的核心查询：用户的 `roles: Vec<String>` 经此一次性翻译为 `permissions: Vec<String>`。
    /// 空入参直接返回空集合，不打 DB。
    pub async fn list_permissions_for_roles(
        &self,
        pool: &Pool<Postgres>,
        roles: &[String],
    ) -> Result<Vec<String>> {
        if roles.is_empty() {
            return Ok(Vec::new());
        }
        // 使用 ANY 绑定数组而不是动态 IN 列表，避免 SQL 注入风险与参数计数烦恼
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"SELECT DISTINCT permission_code FROM role_permissions
               WHERE role = ANY($1) AND deleted_at IS NULL"#,
        )
        .bind(roles)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// 列出指定角色的全部映射明细（admin UI 用）。
    pub async fn list_by_role(
        &self,
        pool: &Pool<Postgres>,
        role: &str,
    ) -> Result<Vec<RolePermission>> {
        let rows: Vec<RolePermissionSchema> = sqlx::query_as(
            r#"SELECT id, role, permission_code, created FROM role_permissions
               WHERE role = $1 AND deleted_at IS NULL
               ORDER BY permission_code ASC"#,
        )
        .bind(role)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(RolePermission::from).collect())
    }
}
