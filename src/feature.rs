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

use crate::cache::get_redis_cache;
use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{get, put};
use serde::Deserialize;
use tibba_error::Error;
use tibba_feature::{FeatureFlag, FeatureFlags};
use tibba_session::AdminSession;

type Result<T> = std::result::Result<T, Error>;

/// 设置开关的请求体。
#[derive(Debug, Deserialize)]
struct SetFlagBody {
    /// 目标开关态
    enabled: bool,
}

/// `GET /features` —— 列出全部开关（Admin）。
async fn list_flags(_admin: AdminSession) -> Result<Json<Vec<FeatureFlag>>> {
    let flags = FeatureFlags::new(get_redis_cache()).list().await?;
    Ok(Json(flags))
}

/// `PUT /features/{name}` —— 设置某开关（Admin）。
async fn set_flag(
    _admin: AdminSession,
    Path(name): Path<String>,
    Json(body): Json<SetFlagBody>,
) -> Result<Json<FeatureFlag>> {
    FeatureFlags::new(get_redis_cache())
        .set(name.clone(), body.enabled)
        .await?;
    Ok(Json(FeatureFlag {
        name,
        enabled: body.enabled,
    }))
}

/// `DELETE /features/{name}` —— 删除某开关（Admin），幂等。
async fn delete_flag(_admin: AdminSession, Path(name): Path<String>) -> Result<StatusCode> {
    FeatureFlags::new(get_redis_cache()).remove(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 构造特性开关管理路由（由 `router.rs` 以 `/features` 前缀挂载）。
pub fn new_feature_router() -> Router {
    Router::new()
        .route("/", get(list_flags))
        .route("/{name}", put(set_flag).delete(delete_flag))
}
