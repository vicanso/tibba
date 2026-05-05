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
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use tibba_error::Error;
use tibba_hook::BoxFuture;
use tibba_model::{Model, ModelListParams, SchemaOption, SchemaView};

type Result<T> = std::result::Result<T, Error>;

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
        Box::pin(async move {
            self.0
                .list_and_count(pool, count, params)
                .await
                .map_err(Error::from)
        })
    }

    fn get_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        Box::pin(async move {
            let result = self.0.get_by_id(pool, id).await.map_err(Error::from)?;
            match result {
                Some(v) => {
                    let json = serde_json::to_value(v).map_err(Error::new)?;
                    Ok(Some(json))
                }
                None => Ok(None),
            }
        })
    }

    fn delete_by_id<'a>(&'a self, pool: &'static PgPool, id: u64) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.0.delete_by_id(pool, id).await.map_err(Error::from) })
    }

    fn update_by_id<'a>(
        &'a self,
        pool: &'static PgPool,
        id: u64,
        data: Value,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0
                .update_by_id(pool, id, data)
                .await
                .map_err(Error::from)
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
            self.0.insert(pool, data).await.map_err(Error::from)
        })
    }

    fn search_options<'a>(
        &'a self,
        pool: &'static PgPool,
        keyword: Option<String>,
    ) -> BoxFuture<'a, Result<Vec<SchemaOption>>> {
        Box::pin(async move {
            self.0
                .search_options(pool, keyword)
                .await
                .map_err(Error::from)
        })
    }
}

static MODEL_REGISTRY: LazyLock<RwLock<HashMap<String, Arc<dyn DynModel>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// 向全局注册表注册一个具名模型。
pub fn register_model(name: impl Into<String>, model: Arc<dyn DynModel>) {
    if let Ok(mut registry) = MODEL_REGISTRY.write() {
        registry.insert(name.into(), model);
    }
}

pub(crate) fn get_registered_model(name: &str) -> Result<Arc<dyn DynModel>> {
    MODEL_REGISTRY
        .read()
        .map_err(|_| Error::new("model registry lock poisoned"))?
        .get(name)
        .cloned()
        .ok_or_else(|| Error::new("The model is not supported"))
}

mod router;
pub use router::*;
