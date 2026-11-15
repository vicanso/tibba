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

//! 后台通用模型接口中的 `user` 模型：写操作后撤销该用户的已登录会话。
//!
//! Session 缓存了登录时的 roles / groups / permissions。此前后台禁用账号或收回
//! 角色后，被处置的用户仍可凭已有会话按旧身份操作，直到 Session TTL 到期。

use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use tibba_cache::RedisCache;
use tibba_error::Error;
use tibba_model::{Model, ModelListParams, SchemaOption, SchemaView, UserModel};
use tibba_router_model::{BoxFuture, DynModel, ModelAdapter, ModelCaller};
use tibba_session::{Role, SessionParams, revoke_user_sessions};

type Result<T> = std::result::Result<T, Error>;

const ERROR_CATEGORY: &str = "user_admin";

/// 这些字段变化会改变用户的身份或授权，必须让旧会话失效。
const IDENTITY_FIELDS: &[&str] = &["status", "roles", "groups", "account", "password"];

/// 更新载荷是否触及身份 / 授权相关字段。
fn touches_identity(data: &Value) -> bool {
    data.as_object()
        .is_some_and(|obj| IDENTITY_FIELDS.iter().any(|field| obj.contains_key(*field)))
}

fn forbidden(sub_category: &str, message: &str) -> Error {
    Error::new(message)
        .with_category(ERROR_CATEGORY)
        .with_sub_category(sub_category)
        .with_status(403)
        .with_exception(false)
}

fn is_self_target(caller: &ModelCaller, target_id: u64) -> bool {
    caller.user_id > 0 && u64::try_from(caller.user_id).ok() == Some(target_id)
}

/// 判断更新载荷是否包含 `su` 超级管理员角色授予。
fn grants_super_admin(data: &Value) -> bool {
    let Some(roles) = data.get("roles") else {
        return false;
    };
    if let Some(arr) = roles.as_array() {
        return arr.iter().any(|item| {
            item.as_str()
                .is_some_and(|r| Role::from(r) == Role::SuperAdmin)
        });
    }
    if let Some(s) = roles.as_str() {
        return s
            .split(',')
            .any(|part| Role::from(part.trim()) == Role::SuperAdmin);
    }
    false
}

/// 判断更新载荷是否将用户状态修改为非启用态（`status != 1`）。
fn disables_account(data: &Value) -> bool {
    let Some(val) = data.get("status") else {
        return false;
    };
    if let Some(num) = val.as_i64() {
        return num != 1;
    }
    if let Some(s) = val.as_str() {
        return s.trim() != "1";
    }
    !val.is_null()
}

fn target_is_super_admin(roles: Option<&[String]>) -> bool {
    roles
        .unwrap_or_default()
        .iter()
        .any(|r| Role::from(r.as_str()) == Role::SuperAdmin)
}

/// 校验创建/更新/删除用户的越权与自我破坏操作。
fn check_create_guard(caller: &ModelCaller, data: &Value) -> Result<()> {
    if !caller.is_super_admin() && grants_super_admin(data) {
        return Err(forbidden(
            "privilege_escalation",
            "only super admin can assign su role",
        ));
    }
    Ok(())
}

fn check_delete_guard(
    caller: &ModelCaller,
    id: u64,
    target_roles: Option<&[String]>,
) -> Result<()> {
    if is_self_target(caller, id) {
        return Err(forbidden(
            "self_operation_forbidden",
            "cannot delete your own account",
        ));
    }
    if !caller.is_super_admin() && target_is_super_admin(target_roles) {
        return Err(forbidden(
            "privilege_escalation",
            "only super admin can delete su account",
        ));
    }
    Ok(())
}

fn check_update_guard(
    caller: &ModelCaller,
    id: u64,
    data: &Value,
    target_roles: Option<&[String]>,
) -> Result<()> {
    if is_self_target(caller, id) && disables_account(data) {
        return Err(forbidden(
            "self_operation_forbidden",
            "cannot disable your own account",
        ));
    }
    if !caller.is_super_admin() && (grants_super_admin(data) || target_is_super_admin(target_roles))
    {
        return Err(forbidden(
            "privilege_escalation",
            "only super admin can manage su role or su account",
        ));
    }
    Ok(())
}

/// 包装 [`ModelAdapter<UserModel>`]：更新身份字段或删除用户后撤销其会话。
pub struct UserAdminModel {
    inner: ModelAdapter<UserModel>,
    cache: &'static RedisCache,
    session_params: Arc<SessionParams>,
}

impl UserAdminModel {
    pub fn new(cache: &'static RedisCache, session_params: Arc<SessionParams>) -> Self {
        Self {
            inner: ModelAdapter(UserModel::new()),
            cache,
            session_params,
        }
    }

    async fn revoke(&self, id: u64) -> Result<()> {
        revoke_user_sessions(self.cache, &self.session_params, id as i64).await
    }
}

impl DynModel for UserAdminModel {
    fn schema_view<'a>(&'a self, pool: &'static PgPool) -> BoxFuture<'a, SchemaView> {
        self.inner.schema_view(pool)
    }

    fn list_and_count<'a>(
        &'a self,
        pool: &'static PgPool,
        count: bool,
        params: &'a ModelListParams,
    ) -> BoxFuture<'a, Result<Value>> {
        self.inner.list_and_count(pool, count, params)
    }

    fn get_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        self.inner.get_by_id(pool, id)
    }

    fn delete_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
        caller: &'a ModelCaller,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let target = UserModel::new().get_by_id(pool, id).await?;
            check_delete_guard(caller, id, target.as_ref().and_then(|u| u.roles.as_deref()))?;
            self.inner.delete_by_id(pool, id, caller).await?;
            self.revoke(id).await
        })
    }

    fn update_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
        data: Value,
        caller: &'a ModelCaller,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let target = if caller.is_super_admin() {
                None
            } else {
                UserModel::new().get_by_id(pool, id).await?
            };
            check_update_guard(
                caller,
                id,
                &data,
                target.as_ref().and_then(|u| u.roles.as_deref()),
            )?;
            let revoke = touches_identity(&data);
            self.inner.update_by_id(pool, id, data, caller).await?;
            if revoke {
                self.revoke(id).await?;
            }
            Ok(())
        })
    }

    fn insert<'a>(
        &'a self,
        pool: &'static PgPool,
        data: Value,
        caller: &'a ModelCaller,
    ) -> BoxFuture<'a, Result<u64>> {
        Box::pin(async move {
            check_create_guard(caller, &data)?;
            self.inner.insert(pool, data, caller).await
        })
    }

    fn search_options<'a>(
        &'a self,
        pool: &'static PgPool,
        keyword: Option<String>,
    ) -> BoxFuture<'a, Result<Vec<SchemaOption>>> {
        self.inner.search_options(pool, keyword)
    }
}

#[cfg(test)]
mod tests {
    use super::{check_create_guard, check_delete_guard, check_update_guard, touches_identity};
    use serde_json::json;
    use tibba_router_model::ModelCaller;

    fn admin_caller(user_id: i64) -> ModelCaller {
        ModelCaller {
            user_id,
            account: "admin1".to_string(),
            roles: vec!["admin".to_string()],
        }
    }

    fn su_caller(user_id: i64) -> ModelCaller {
        ModelCaller {
            user_id,
            account: "root".to_string(),
            roles: vec!["su".to_string()],
        }
    }

    #[test]
    fn only_identity_changes_trigger_revocation() {
        assert!(touches_identity(&json!({ "status": 0 })));
        assert!(touches_identity(
            &json!({ "roles": ["admin"], "remark": "x" })
        ));
        assert!(!touches_identity(
            &json!({ "nickname": "tree", "remark": "x" })
        ));
        assert!(!touches_identity(&json!(null)));
    }

    #[test]
    fn guards_block_self_harm_and_non_su_privilege_escalation() {
        let admin = admin_caller(10);
        let su = su_caller(1);

        // 禁止自删与自禁
        let err = check_delete_guard(&admin, 10, None).expect_err("self delete must fail");
        assert_eq!(err.status(), 403);
        let err = check_update_guard(&admin, 10, &json!({ "status": 0 }), None)
            .expect_err("self disable must fail");
        assert_eq!(err.status(), 403);
        assert!(check_update_guard(&admin, 10, &json!({ "status": 1 }), None).is_ok());

        // 非 su 不得创建/修改/删除 su，也不得授予 su 角色
        let err = check_create_guard(&admin, &json!({ "roles": ["su"] }))
            .expect_err("non-su create su must fail");
        assert_eq!(err.status(), 403);

        let su_roles = vec!["su".to_string()];
        assert_eq!(
            check_update_guard(&admin, 1, &json!({ "status": 0 }), Some(&su_roles))
                .expect_err("non-su edit su must fail")
                .status(),
            403
        );
        assert_eq!(
            check_delete_guard(&admin, 1, Some(&su_roles))
                .expect_err("non-su delete su must fail")
                .status(),
            403
        );

        // su 可以管理其它 su 账号（仍禁止自删）
        assert!(check_update_guard(&su, 2, &json!({ "roles": ["su"] }), Some(&su_roles)).is_ok());
        assert_eq!(
            check_delete_guard(&su, 1, Some(&su_roles))
                .expect_err("su self delete blocked")
                .status(),
            403
        );
    }
}
