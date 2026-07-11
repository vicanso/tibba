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

use serde::Serialize;
use serde_json::Value;
use snafu::{OptionExt, ResultExt, Snafu};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use tibba_error::Error as BaseError;
use tibba_hook::BoxFuture;
use tibba_model::{Model, ModelListParams, SchemaOption, SchemaView};

/// 模块对外暴露的 Result 仍以 `tibba_error::Error` 为错误类型，本地 `Error` 仅作 snafu 上下文。
type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "model_router";

/// 模型路由模块内部错误，统一通过 `From` 转换为 `tibba_error::Error`。
#[derive(Debug, Snafu)]
pub(crate) enum Error {
    /// 全局模型注册表读锁中毒，通常表示先前写入时 panic 留下了不一致状态
    #[snafu(display("model registry lock poisoned"))]
    RegistryPoisoned,

    /// 调用方请求的模型名未注册（HTTP 404）
    #[snafu(display("The model is not supported: {name}"))]
    ModelNotSupported { name: String },

    /// 根据 id 查询不到记录（HTTP 404）
    #[snafu(display("The record is not found: model={model}, id={id}"))]
    RecordNotFound { model: String, id: u64 },

    /// 模型行序列化为 JSON 失败
    #[snafu(display("model row to JSON fail: {source}"))]
    Json { source: serde_json::Error },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::RegistryPoisoned => BaseError::new("model registry lock poisoned")
                .with_sub_category("registry_poisoned")
                .with_exception(true),
            Error::ModelNotSupported { name } => {
                BaseError::new(format!("The model is not supported: {name}"))
                    .with_sub_category("model_not_supported")
                    .with_status(404)
                    .with_exception(false)
            }
            Error::RecordNotFound { model, id } => {
                BaseError::new(format!("The record is not found: model={model}, id={id}"))
                    .with_sub_category("record_not_found")
                    .with_status(404)
                    .with_exception(false)
            }
            Error::Json { source } => BaseError::new(source)
                .with_sub_category("json")
                .with_exception(true),
        };
        err.with_category(ERROR_CATEGORY)
    }
}

/// 动态模型 trait，使用 BoxFuture 支持 dyn trait object 场景。
pub trait DynModel: Send + Sync {
    fn schema_view<'a>(&'a self, pool: &'static PgPool) -> BoxFuture<'a, SchemaView>;
    fn list_and_count<'a>(
        &'a self,
        pool: &'static PgPool,
        count: bool,
        params: &'a ModelListParams,
    ) -> BoxFuture<'a, Result<Value>>;
    fn get_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
    ) -> BoxFuture<'a, Result<Option<Value>>>;
    fn delete_by_id<'a>(&'a self, pool: &'static PgPool, id: u64) -> BoxFuture<'a, Result<()>>;
    fn update_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
        data: Value,
    ) -> BoxFuture<'a, Result<()>>;
    /// 创建记录。`caller_id` 为当前操作用户 ID，实现方负责注入所需字段（如 `created_by`）。
    fn insert<'a>(
        &'a self,
        pool: &'static PgPool,
        data: Value,
        caller_id: i64,
    ) -> BoxFuture<'a, Result<u64>>;
    fn search_options<'a>(
        &'a self,
        pool: &'static PgPool,
        keyword: Option<String>,
    ) -> BoxFuture<'a, Result<Vec<SchemaOption>>>;
}

/// 将实现了 [`Model`] trait 的类型适配为 [`DynModel`]。
/// `insert` 时自动向 data 注入 `created_by` 字段，其余字段由模型的 `insert` 实现负责。
///
/// 注意：所有方法依赖 `From<tibba_model::Error> for tibba_error::Error` 完成自动转换，
/// 因此调用侧只需使用 `?`，不必显式 `.map_err`。
pub struct ModelAdapter<M>(pub M);

impl<M> DynModel for ModelAdapter<M>
where
    M: Model + 'static,
    M::Output: Serialize,
{
    fn schema_view<'a>(&'a self, pool: &'static PgPool) -> BoxFuture<'a, SchemaView> {
        Box::pin(async move { self.0.schema_view(pool).await })
    }

    fn list_and_count<'a>(
        &'a self,
        pool: &'static PgPool,
        count: bool,
        params: &'a ModelListParams,
    ) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(self.0.list_and_count(pool, count, params).await?) })
    }

    fn get_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        Box::pin(async move {
            match self.0.get_by_id(pool, id).await? {
                Some(v) => {
                    let json = serde_json::to_value(v).context(JsonSnafu)?;
                    Ok(Some(json))
                }
                None => Ok(None),
            }
        })
    }

    fn delete_by_id<'a>(&'a self, pool: &'static PgPool, id: u64) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.delete_by_id(pool, id).await?;
            Ok(())
        })
    }

    fn update_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
        data: Value,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.update_by_id(pool, id, data).await?;
            Ok(())
        })
    }

    fn insert<'a>(
        &'a self,
        pool: &'static PgPool,
        mut data: Value,
        caller_id: i64,
    ) -> BoxFuture<'a, Result<u64>> {
        Box::pin(async move {
            if let Some(obj) = data.as_object_mut() {
                obj.insert("created_by".to_string(), caller_id.into());
            }
            Ok(self.0.insert(pool, data).await?)
        })
    }

    fn search_options<'a>(
        &'a self,
        pool: &'static PgPool,
        keyword: Option<String>,
    ) -> BoxFuture<'a, Result<Vec<SchemaOption>>> {
        Box::pin(async move { Ok(self.0.search_options(pool, keyword).await?) })
    }
}

/// 模型级权限元数据：按操作细粒度授权，避免「凡 Admin 可改一切表」。
///
/// - `read` / `write` 为 `None`（默认）：沿用 **Admin 角色** 门禁（兼容现网）
/// - 设为权限码（如 `model:user:read`）：要求该码，**或** Admin 角色（运维逃生舱）
/// - SuperAdmin 通常持有 `*`，自动放行所有码
///
/// 链式配置：
/// ```ignore
/// ModelPermissions::default()
///     .with_read("model:user:read")
///     .with_write("model:user:write")
/// ```
#[derive(Debug, Clone, Default)]
pub struct ModelPermissions {
    /// list / detail 所需权限码；`None` → 仅 Admin
    read: Option<&'static str>,
    /// create / update / delete 所需权限码；`None` → 仅 Admin
    write: Option<&'static str>,
}

impl ModelPermissions {
    #[must_use]
    pub fn with_read(mut self, permission: &'static str) -> Self {
        self.read = Some(permission);
        self
    }

    #[must_use]
    pub fn with_write(mut self, permission: &'static str) -> Self {
        self.write = Some(permission);
        self
    }

    /// 读权限码（若有）。
    #[must_use]
    pub fn read(&self) -> Option<&'static str> {
        self.read
    }

    /// 写权限码（若有）。
    #[must_use]
    pub fn write(&self) -> Option<&'static str> {
        self.write
    }
}

/// 注册表条目：动态模型 + 权限元数据。
#[derive(Clone)]
pub struct RegisteredModel {
    pub model: Arc<dyn DynModel>,
    pub permissions: ModelPermissions,
}

static MODEL_REGISTRY: LazyLock<RwLock<HashMap<String, RegisteredModel>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// 向全局注册表注册具名模型，权限为默认（Admin 角色门禁）。
pub fn register_model(name: impl Into<String>, model: Arc<dyn DynModel>) {
    register_model_with(name, model, ModelPermissions::default());
}

/// 注册具名模型并附带读写权限码。
pub fn register_model_with(
    name: impl Into<String>,
    model: Arc<dyn DynModel>,
    permissions: ModelPermissions,
) {
    if let Ok(mut registry) = MODEL_REGISTRY.write() {
        registry.insert(name.into(), RegisteredModel { model, permissions });
    }
}

pub(crate) fn get_registered_model(name: &str) -> Result<RegisteredModel> {
    // `.ok()` 丢弃 PoisonError 携带的读守卫（其内容无诊断价值），转交 snafu 统一上下文
    let registry = MODEL_REGISTRY.read().ok().context(RegistryPoisonedSnafu)?;
    let entry = registry
        .get(name)
        .cloned()
        .context(ModelNotSupportedSnafu {
            name: name.to_string(),
        })?;
    Ok(entry)
}

/// 按注册元数据鉴权。
///
/// - 权限码已配置：持有该码 **或** Admin 角色
/// - 未配置：仅 Admin 角色（与历史 `AdminSession` 一致）
pub(crate) fn authorize_model_access(
    session: &tibba_session::Session,
    permissions: &ModelPermissions,
    write: bool,
) -> Result<()> {
    let code = if write {
        permissions.write()
    } else {
        permissions.read()
    };
    match code {
        // Admin 逃生舱优先：运维角色不依赖细粒度码是否入库
        Some(_) if session.is_admin() => session.require_admin(),
        Some(required) => session.require_permission(required),
        None => session.require_admin(),
    }
}

mod router;
pub use router::*;
