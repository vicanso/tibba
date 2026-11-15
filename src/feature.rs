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

//! 特性开关管理路由（挂载于 `/features`，全部需 Admin 角色）。
//!
//! - `GET    /features`         列出全部开关
//! - `PUT    /features/{name}`  设置某开关（body: `{ "enabled": bool }`）
//! - `DELETE /features/{name}`  删除某开关
//!
//! 服务端要按开关放量时，直接用 [`tibba_feature::FeatureFlags::is_enabled`] 即可，
//! 无需经过这些管理端点。

use crate::app_ctx::get_app_ctx;
use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{get, put};
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::Duration;
use tibba_cache::InvalidationListener;
use tibba_error::Error;
use tibba_feature::{FeatureFlag, FeatureFlags};
use tibba_session::AdminSession;

type Result<T> = std::result::Result<T, Error>;

/// 全局特性开关实例及跨节点失效订阅句柄：
/// 开启 10s 进程内缓存并通过 `"inval:feature_flags"` Pub/Sub 频道实时失效，
/// 把 listener 一并存入 `OnceLock` 保持进程生命周期内持续订阅。
static FEATURE_FLAGS: OnceLock<(FeatureFlags, Option<InvalidationListener>)> = OnceLock::new();

/// 设置开关的请求体。
#[derive(Debug, Deserialize)]
struct SetFlagBody {
    /// 目标开关态
    enabled: bool,
}

/// 获取全局特性开关管理器单例。
pub fn feature_flags() -> &'static FeatureFlags {
    &FEATURE_FLAGS
        .get_or_init(|| {
            let flags = FeatureFlags::new(get_app_ctx().cache)
                .with_local_cache(Duration::from_secs(10))
                .with_instant_invalidation("inval:feature_flags");
            let listener = flags.spawn_invalidation_listener();
            (flags, listener)
        })
        .0
}

/// `GET /features` —— 列出全部开关（Admin）。
async fn list_flags(_admin: AdminSession) -> Result<Json<Vec<FeatureFlag>>> {
    let flags = feature_flags().list().await?;
    Ok(Json(flags))
}

/// `PUT /features/{name}` —— 设置某开关（Admin）。
async fn set_flag(
    _admin: AdminSession,
    Path(name): Path<String>,
    Json(body): Json<SetFlagBody>,
) -> Result<Json<FeatureFlag>> {
    feature_flags().set(name.clone(), body.enabled).await?;
    Ok(Json(FeatureFlag {
        name,
        enabled: body.enabled,
    }))
}

/// `DELETE /features/{name}` —— 删除某开关（Admin），幂等。
async fn delete_flag(_admin: AdminSession, Path(name): Path<String>) -> Result<StatusCode> {
    feature_flags().remove(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 构造特性开关管理路由（由 `router.rs` 以 `/features` 前缀挂载）。
pub fn new_feature_router() -> Router {
    Router::new()
        .route("/", get(list_flags))
        .route("/{name}", put(set_flag).delete(delete_flag))
}
