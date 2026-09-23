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
use tibba_router_model::{DynModel, ModelAdapter};
use tibba_runtime::BoxFuture;
use tibba_session::{SessionParams, revoke_user_sessions};

type Result<T> = std::result::Result<T, Error>;

/// 这些字段变化会改变用户的身份或授权，必须让旧会话失效。
const IDENTITY_FIELDS: &[&str] = &["status", "roles", "groups", "account", "password"];

/// 更新载荷是否触及身份 / 授权相关字段。
fn touches_identity(data: &Value) -> bool {
    data.as_object()
        .is_some_and(|obj| IDENTITY_FIELDS.iter().any(|field| obj.contains_key(*field)))
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

    fn delete_by_id<'a>(&'a self, pool: &'static PgPool, id: u64) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.inner.delete_by_id(pool, id).await?;
            self.revoke(id).await
        })
    }

    fn update_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
        data: Value,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let revoke = touches_identity(&data);
            self.inner.update_by_id(pool, id, data).await?;
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
        caller_id: i64,
    ) -> BoxFuture<'a, Result<u64>> {
        self.inner.insert(pool, data, caller_id)
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
    use super::touches_identity;
    use serde_json::json;

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
}
