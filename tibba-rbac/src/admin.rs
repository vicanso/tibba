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

use sqlx::PgPool;
use std::sync::Arc;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_model_builtin::{PermissionModel, RolePermissionModel};
use tibba_session::{SessionParams, revoke_role_sessions};

type Result<T, E = BaseError> = std::result::Result<T, E>;

/// 角色权限管理：写库之后同步处理已签发凭证。
///
/// Session / JWT 在签发时缓存了权限并集。收回权限（撤销授予、删除权限点）后
/// 若不处理，持有该角色的用户在凭证过期前（Session 默认 7 天）仍保有被收回的
/// 权限。这里在收回成功后撤销受影响角色的凭证，相关用户需重新登录。
///
/// 授予权限不撤销凭证：不涉及安全收窄，新权限在下次登录时生效，避免一次授权
/// 就把整个角色的用户全部踢下线。
pub struct RbacAdmin {
    pool: &'static PgPool,
    cache: &'static RedisCache,
    session_params: Arc<SessionParams>,
}

impl RbacAdmin {
    pub fn new(
        pool: &'static PgPool,
        cache: &'static RedisCache,
        session_params: Arc<SessionParams>,
    ) -> Self {
        Self {
            pool,
            cache,
            session_params,
        }
    }

    /// 授予角色权限码（幂等）。已登录用户在重新登录后获得新权限。
    pub async fn grant(&self, role: &str, permission_code: &str) -> Result<()> {
        RolePermissionModel::new()
            .grant(self.pool, role, permission_code)
            .await?;
        Ok(())
    }

    /// 收回角色的权限码；确有收回时撤销该角色的已登录凭证。返回是否收回。
    pub async fn revoke(&self, role: &str, permission_code: &str) -> Result<bool> {
        let affected = RolePermissionModel::new()
            .revoke(self.pool, role, permission_code)
            .await?;
        if affected == 0 {
            return Ok(false);
        }
        revoke_role_sessions(self.cache, &self.session_params, role).await?;
        Ok(true)
    }

    /// 删除权限点并级联收回所有角色的授予，撤销这些角色的已登录凭证。
    /// 返回被收回该权限的角色列表。
    pub async fn delete_permission(&self, code: &str) -> Result<Vec<String>> {
        let roles = PermissionModel::new()
            .soft_delete_by_code(self.pool, code)
            .await?;
        for role in &roles {
            revoke_role_sessions(self.cache, &self.session_params, role).await?;
        }
        Ok(roles)
    }
}
