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

//! 动态配置运行期读取（双层缓存 + 跨节点 pub/sub 失效联动）、后台变更自动失效包装与公开白名单查询路由。

use crate::model::{BoxFuture, DynModel, ModelAdapter, ModelCaller};
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::PgPool;
use std::fmt;
use std::num::NonZeroUsize;
use std::time::Duration;
use tibba_cache::{InvalidationBus, InvalidationListener, RedisCache, RedisClient, TwoLevelStore};
use tibba_error::Error as BaseError;
use tibba_model::{
    Configuration, ConfigurationModel, Model, ModelListParams, SchemaOption, SchemaView,
    now_primitive_utc,
};
use tibba_util::JsonResult;
use tracing::warn;
use utoipa::{OpenApi, ToSchema};

type Result<T, E = BaseError> = std::result::Result<T, E>;

const LOG_TARGET: &str = "tibba:config_store";
const ERROR_CATEGORY: &str = "configuration";

const ITEM_L1_CAP: NonZeroUsize = match NonZeroUsize::new(512) {
    Some(v) => v,
    None => NonZeroUsize::MIN,
};
const CATEGORY_L1_CAP: NonZeroUsize = match NonZeroUsize::new(64) {
    Some(v) => v,
    None => NonZeroUsize::MIN,
};
const CONFIG_L1_TTL: Duration = Duration::from_secs(30);
const CONFIG_L2_TTL: Duration = Duration::from_secs(600);
const MAX_SEGMENT_LEN: usize = 100;

/// 允许通过匿名 HTTP 接口读取的配置分类白名单（屏蔽 system / alarm / user / response_headers / sql_query 等内部分类）。
pub const PUBLIC_CONFIG_CATEGORIES: &[&str] = &["common", "app"];

/// 运行期配置化只读 SQL 查询占用的 `configurations.category` 常量（仅限 `su` 超管读写）。
pub const SQL_QUERY_CATEGORY: &str = "sql_query";

/// 运行期配置化 SQL 查询分组占用的 `configurations.category` 常量（仅限 `su` 或 `admin` 管理）。
pub const SQL_QUERY_GROUP_CATEGORY: &str = "sql_query_group";

/// 应用签名凭证（`app_id` / 加密 `app_secret` / 权限列表）占用的 `configurations.category` 常量（强制走 `/apps` 专用端点）。
pub const APP_CLIENT_CATEGORY: &str = "app_client";

/// 判断配置分类是否属于对外公开白名单。
#[must_use]
pub fn is_public_category(category: &str) -> bool {
    PUBLIC_CONFIG_CATEGORIES.contains(&category)
}

/// 保护 `sql_query`（仅 `su`）、`sql_query_group`（仅 `su`/`admin`）与 `app_client`（禁止通用配置接口直写）。
pub(crate) fn check_sql_query_category_guard(
    is_super_admin: bool,
    is_admin: bool,
    category: Option<&str>,
) -> Result<()> {
    if category == Some(SQL_QUERY_CATEGORY) && !is_super_admin {
        return Err(
            BaseError::new("only super admin can modify sql_query configurations")
                .with_category(ERROR_CATEGORY)
                .with_sub_category("privilege_escalation")
                .with_status(403)
                .with_exception(false),
        );
    }
    if category == Some(SQL_QUERY_GROUP_CATEGORY) && !is_admin {
        return Err(BaseError::new(
            "only admin or super admin can modify sql_query_group configurations",
        )
        .with_category(ERROR_CATEGORY)
        .with_sub_category("privilege_escalation")
        .with_status(403)
        .with_exception(false));
    }
    if category == Some(APP_CLIENT_CATEGORY) {
        return Err(
            BaseError::new("app_client configurations must be managed via /apps")
                .with_category(ERROR_CATEGORY)
                .with_sub_category("forbidden_category")
                .with_status(403)
                .with_exception(false),
        );
    }
    Ok(())
}

/// 在通用模型管理响应中脱敏 `app_client` 密文，避免 `secret_ciphertext` 外泄到管理接口。
pub(crate) fn redact_app_client_json(val: &mut Value) {
    fn redact_row(row: &mut Value) {
        let Some(obj) = row.as_object_mut() else {
            return;
        };
        if obj.get("category").and_then(Value::as_str) != Some(APP_CLIENT_CATEGORY) {
            return;
        }
        if let Some(data_obj) = obj.get_mut("data").and_then(Value::as_object_mut)
            && data_obj.contains_key("secret_ciphertext")
        {
            data_obj.insert(
                "secret_ciphertext".to_string(),
                Value::String("<redacted>".to_string()),
            );
        }
    }

    if let Some(items) = val.get_mut("configurations").and_then(Value::as_array_mut) {
        for item in items {
            redact_row(item);
        }
    } else {
        redact_row(val);
    }
}

fn validate_segment(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_SEGMENT_LEN
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(BaseError::new(format!("invalid config {field}"))
            .with_category(ERROR_CATEGORY)
            .with_sub_category("invalid_param")
            .with_status(400)
            .with_exception(false));
    }
    Ok(())
}

fn ensure_public_category(category: &str) -> Result<()> {
    validate_segment(category, "category")?;
    if !is_public_category(category) {
        return Err(
            BaseError::new(format!("config category '{category}' is not public"))
                .with_category(ERROR_CATEGORY)
                .with_sub_category("forbidden_category")
                .with_status(403)
                .with_exception(false),
        );
    }
    Ok(())
}

/// 动态配置运行期双层缓存读取器（L1 进程内 + L2 Redis + pub/sub 跨节点失效广播）。
pub struct ConfigStore {
    pool: &'static PgPool,
    item_store: TwoLevelStore<Option<Configuration>>,
    category_store: TwoLevelStore<Vec<Configuration>>,
    item_listener: Option<InvalidationListener>,
    category_listener: Option<InvalidationListener>,
}

impl fmt::Debug for ConfigStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigStore").finish_non_exhaustive()
    }
}

impl ConfigStore {
    /// 构造配置缓存并启动跨节点失效订阅任务。
    pub fn new(pool: &'static PgPool, client: &'static RedisClient) -> Self {
        let item_cache = RedisCache::new(client).with_prefix("cfg:item:");
        let item_store = TwoLevelStore::new(item_cache, ITEM_L1_CAP, CONFIG_L1_TTL)
            .with_l2_ttl(CONFIG_L2_TTL)
            .with_invalidation(InvalidationBus::new(client, "inval:cfg:item"));
        let item_listener = item_store.spawn_invalidation_listener();

        let category_cache = RedisCache::new(client).with_prefix("cfg:cat:");
        let category_store = TwoLevelStore::new(category_cache, CATEGORY_L1_CAP, CONFIG_L1_TTL)
            .with_l2_ttl(CONFIG_L2_TTL)
            .with_invalidation(InvalidationBus::new(client, "inval:cfg:cat"));
        let category_listener = category_store.spawn_invalidation_listener();

        Self {
            pool,
            item_store,
            category_store,
            item_listener,
            category_listener,
        }
    }

    /// 停止跨节点失效广播监听任务（优雅停机时调用）。
    pub fn stop(&self) {
        if let Some(listener) = &self.item_listener {
            listener.stop();
        }
        if let Some(listener) = &self.category_listener {
            listener.stop();
        }
    }

    fn item_key(category: &str, name: &str) -> String {
        format!("{category}:{name}")
    }

    /// 读取指定 `(category, name)` 当前生效的配置；未配置、已禁用或不在生效时间窗内时返回 `None`。
    pub async fn get_effective(&self, category: &str, name: &str) -> Result<Option<Configuration>> {
        let pool = self.pool;
        let cat = category.to_string();
        let cfg_name = name.to_string();
        let key = Self::item_key(category, name);
        let cached = self
            .item_store
            .get_or_set(&key, || async move {
                Ok(ConfigurationModel::new()
                    .get_by_category_name(pool, &cat, &cfg_name)
                    .await?)
            })
            .await?;

        let now = now_primitive_utc();
        Ok(cached.filter(|cfg| cfg.is_effective_at(now)))
    }

    /// 读取当前生效配置的 JSON `data` 字段。
    pub async fn get_json(&self, category: &str, name: &str) -> Result<Option<Value>> {
        Ok(self.get_effective(category, name).await?.map(|c| c.data))
    }

    /// 读取当前生效配置并反序列化为目标类型。
    pub async fn get_typed<T: DeserializeOwned>(
        &self,
        category: &str,
        name: &str,
    ) -> Result<Option<T>> {
        let Some(val) = self.get_json(category, name).await? else {
            return Ok(None);
        };
        serde_json::from_value::<T>(val).map(Some).map_err(|e| {
            BaseError::new(e)
                .with_category(ERROR_CATEGORY)
                .with_sub_category("stored_json")
                .with_status(500)
                .with_exception(true)
        })
    }

    /// 读取某分类下当前全部生效的配置列表（按 name 升序）。
    pub async fn list_effective_by_category(&self, category: &str) -> Result<Vec<Configuration>> {
        let pool = self.pool;
        let cat = category.to_string();
        let items = self
            .category_store
            .get_or_set(category, || async move {
                Ok(ConfigurationModel::new()
                    .list_by_category(pool, &cat)
                    .await?)
            })
            .await?;

        let now = now_primitive_utc();
        Ok(items
            .into_iter()
            .filter(|cfg| cfg.is_effective_at(now))
            .collect())
    }

    /// 清除指定单项及所属分类的 L1/L2 缓存并广播给其余实例（best-effort）。
    pub async fn invalidate(&self, category: &str, name: &str) {
        let key = Self::item_key(category, name);
        if let Err(err) = self.item_store.del(&key).await {
            warn!(target: LOG_TARGET, key, error = %err, "invalidate item config cache failed");
        }
        if let Err(err) = self.category_store.del(category).await {
            warn!(target: LOG_TARGET, category, error = %err, "invalidate category config cache failed");
        }
    }
}

/// 后台通用模型接口中的 `configuration` 模型包装：增删改后自动清理 `ConfigStore` 并广播跨节点失效。
pub struct ConfigurationAdminModel {
    inner: ModelAdapter<ConfigurationModel>,
    store: &'static ConfigStore,
}

impl ConfigurationAdminModel {
    pub fn new(store: &'static ConfigStore) -> Self {
        Self {
            inner: ModelAdapter(ConfigurationModel::new()),
            store,
        }
    }
}

impl DynModel for ConfigurationAdminModel {
    fn schema_view<'a>(&'a self, pool: &'static PgPool) -> BoxFuture<'a, SchemaView> {
        self.inner.schema_view(pool)
    }

    fn list_and_count<'a>(
        &'a self,
        pool: &'static PgPool,
        count: bool,
        params: &'a ModelListParams,
    ) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let mut val = self.inner.list_and_count(pool, count, params).await?;
            redact_app_client_json(&mut val);
            Ok(val)
        })
    }

    fn get_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        Box::pin(async move {
            let mut opt = self.inner.get_by_id(pool, id).await?;
            if let Some(val) = &mut opt {
                redact_app_client_json(val);
            }
            Ok(opt)
        })
    }

    fn delete_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
        caller: &'a ModelCaller,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let existing = ConfigurationModel::new().get_by_id(pool, id).await?;
            check_sql_query_category_guard(
                caller.is_super_admin(),
                caller.is_admin(),
                existing.as_ref().map(|cfg| cfg.category.as_str()),
            )?;
            self.inner.delete_by_id(pool, id, caller).await?;
            if let Some(cfg) = existing {
                self.store.invalidate(&cfg.category, &cfg.name).await;
            }
            Ok(())
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
            let existing = ConfigurationModel::new().get_by_id(pool, id).await?;
            let is_su = caller.is_super_admin();
            let is_adm = caller.is_admin();
            check_sql_query_category_guard(
                is_su,
                is_adm,
                existing.as_ref().map(|cfg| cfg.category.as_str()),
            )?;
            check_sql_query_category_guard(
                is_su,
                is_adm,
                data.get("category").and_then(Value::as_str),
            )?;

            self.inner.update_by_id(pool, id, data, caller).await?;
            if let Some(prev) = existing {
                self.store.invalidate(&prev.category, &prev.name).await;
            }
            if let Ok(Some(cfg)) = ConfigurationModel::new().get_by_id(pool, id).await {
                self.store.invalidate(&cfg.category, &cfg.name).await;
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
            check_sql_query_category_guard(
                caller.is_super_admin(),
                caller.is_admin(),
                data.get("category").and_then(Value::as_str),
            )?;
            let id = self.inner.insert(pool, data, caller).await?;
            if let Ok(Some(cfg)) = ConfigurationModel::new().get_by_id(pool, id).await {
                self.store.invalidate(&cfg.category, &cfg.name).await;
            }
            Ok(id)
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

/// 对外公开的生效配置条目响应。
#[derive(Debug, Serialize, ToSchema)]
struct PublicConfigResp {
    /// 配置分类（仅限公开白名单 `common` / `app`）
    category: String,
    /// 配置名称
    name: String,
    /// 配置内容 JSON
    #[schema(value_type = Object)]
    data: Value,
    /// 最后修改时间（RFC3339）
    modified: String,
}

impl From<Configuration> for PublicConfigResp {
    fn from(cfg: Configuration) -> Self {
        Self {
            category: cfg.category,
            name: cfg.name,
            data: cfg.data,
            modified: cfg.modified,
        }
    }
}

/// 按分类读取当前生效的公开配置列表（仅限 `common` / `app`）。
#[utoipa::path(
    get,
    path = "/commons/configs/{category}",
    tag = "common",
    params(("category" = String, Path, description = "配置分类（仅允许 common 或 app）")),
    responses(
        (status = 200, description = "当前生效的公开配置列表", body = Vec<PublicConfigResp>),
        (status = 403, description = "禁止公开访问该配置分类")
    )
)]
async fn list_public_configs(
    State(store): State<&'static ConfigStore>,
    Path(category): Path<String>,
) -> JsonResult<Vec<PublicConfigResp>> {
    ensure_public_category(&category)?;
    let items = store.list_effective_by_category(&category).await?;
    Ok(Json(
        items.into_iter().map(PublicConfigResp::from).collect(),
    ))
}

/// 按分类与名称读取单条当前生效的公开配置（仅限 `common` / `app`）。
#[utoipa::path(
    get,
    path = "/commons/configs/{category}/{name}",
    tag = "common",
    params(
        ("category" = String, Path, description = "配置分类（仅允许 common 或 app）"),
        ("name" = String, Path, description = "配置名称")
    ),
    responses(
        (status = 200, description = "当前生效的公开配置", body = PublicConfigResp),
        (status = 403, description = "禁止公开访问该配置分类"),
        (status = 404, description = "配置不存在或当前未生效")
    )
)]
async fn get_public_config(
    State(store): State<&'static ConfigStore>,
    Path((category, name)): Path<(String, String)>,
) -> JsonResult<PublicConfigResp> {
    ensure_public_category(&category)?;
    validate_segment(&name, "name")?;
    let cfg = store
        .get_effective(&category, &name)
        .await?
        .ok_or_else(|| {
            BaseError::new(format!("config not found or inactive: {category}/{name}"))
                .with_category(ERROR_CATEGORY)
                .with_sub_category("config_not_found")
                .with_status(404)
                .with_exception(false)
        })?;
    Ok(Json(PublicConfigResp::from(cfg)))
}

/// 构建公开只读动态配置路由（挂载于 `/commons/configs/*`）。
pub fn new_config_router(store: &'static ConfigStore) -> Router {
    Router::new()
        .route("/commons/configs/{category}", get(list_public_configs))
        .route("/commons/configs/{category}/{name}", get(get_public_config))
        .with_state(store)
}

#[derive(OpenApi)]
#[openapi(
    paths(list_public_configs, get_public_config),
    components(schemas(PublicConfigResp))
)]
struct ConfigApiDoc;

/// 返回公开配置路由的 OpenAPI 文档片段。
pub fn openapi() -> utoipa::openapi::OpenApi {
    ConfigApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_category_whitelist_blocks_internal_categories() {
        assert!(ensure_public_category("common").is_ok());
        assert!(ensure_public_category("app").is_ok());

        for blocked in ["system", "alarm", "user", "response_headers"] {
            let err = ensure_public_category(blocked).expect_err("internal category must fail");
            assert_eq!(err.status(), 403);
            assert_eq!(err.sub_category(), Some("forbidden_category"));
            assert!(!err.is_exception());
        }
    }

    #[test]
    fn validate_segment_rejects_bad_inputs() {
        assert!(validate_segment("app_theme.v1-dark", "name").is_ok());
        for bad in [
            "",
            "bad/name",
            "bad space",
            &"a".repeat(MAX_SEGMENT_LEN + 1),
        ] {
            let err = validate_segment(bad, "name").expect_err("bad segment must fail");
            assert_eq!(err.status(), 400);
        }
    }

    #[test]
    fn sql_query_category_guard_requires_super_admin() {
        assert!(check_sql_query_category_guard(false, false, Some("app")).is_ok());
        assert!(check_sql_query_category_guard(false, false, Some("common")).is_ok());
        assert!(check_sql_query_category_guard(false, false, None).is_ok());
        assert!(check_sql_query_category_guard(true, true, Some(SQL_QUERY_CATEGORY)).is_ok());
        assert!(
            check_sql_query_category_guard(false, true, Some(SQL_QUERY_GROUP_CATEGORY)).is_ok()
        );
        assert!(check_sql_query_category_guard(true, true, Some(SQL_QUERY_GROUP_CATEGORY)).is_ok());

        let err = check_sql_query_category_guard(false, true, Some(SQL_QUERY_CATEGORY))
            .expect_err("non-su modifying sql_query must be rejected");
        assert_eq!(err.status(), 403);
        assert_eq!(err.sub_category(), Some("privilege_escalation"));
        assert!(!err.is_exception());

        let group_err =
            check_sql_query_category_guard(false, false, Some(SQL_QUERY_GROUP_CATEGORY))
                .expect_err("non-admin modifying sql_query_group must be rejected");
        assert_eq!(group_err.status(), 403);
        assert_eq!(group_err.sub_category(), Some("privilege_escalation"));
        assert!(!group_err.is_exception());

        let app_err = check_sql_query_category_guard(true, true, Some(APP_CLIENT_CATEGORY))
            .expect_err("direct mutation of app_client via generic model must be blocked");
        assert_eq!(app_err.status(), 403);
        assert_eq!(app_err.sub_category(), Some("forbidden_category"));

        let mut list_val = serde_json::json!({
            "configurations": [
                {
                    "id": 1,
                    "category": APP_CLIENT_CATEGORY,
                    "name": "svc_a",
                    "data": { "secret_ciphertext": "secret-blob", "secret_prefix": "abc12345" }
                },
                {
                    "id": 2,
                    "category": "common",
                    "name": "site",
                    "data": { "secret_ciphertext": "keep-other-category" }
                }
            ]
        });
        redact_app_client_json(&mut list_val);
        assert_eq!(
            list_val["configurations"][0]["data"]["secret_ciphertext"],
            "<redacted>"
        );
        assert_eq!(
            list_val["configurations"][0]["data"]["secret_prefix"],
            "abc12345"
        );
        assert_eq!(
            list_val["configurations"][1]["data"]["secret_ciphertext"],
            "keep-other-category"
        );
    }
}
