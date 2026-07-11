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

//! API Key / 个人访问令牌（PAT）：管理端点 + 鉴权中间件。
//!
//! ## 适用场景
//! 「机器对机器」调用（CI / 脚本 / 第三方），区别于浏览器 Cookie Session 与短期 JWT。
//!
//! ## 管理端点（均需登录态，用户管理自己的 key）
//! - `POST   /users/api-keys`       创建（**明文令牌仅此一次返回**）
//! - `GET    /users/api-keys`       列出（不含哈希/明文）
//! - `DELETE /users/api-keys/{id}`  吊销（软删除，幂等）
//!
//! ## 鉴权中间件 [`api_key_auth`]
//! 全局挂载（在 session 中间件之后）。当请求带 `Authorization: Bearer tibba_...`
//! 或 `X-API-Key: tibba_...` 时，校验哈希并**注入一个已登录 Session**到请求扩展，
//! 使下游 `UserSession` / `AdminSession` 提取器对 API Key 请求透明工作。无效令牌
//! 不注入，按未登录处理。

use axum::Json;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tibba_cache::RedisCache;
use tibba_model::{Model, UserModel};
use tibba_model_builtin::{ApiKey, ApiKeyModel, CreateApiKeyParams, RolePermissionModel};
use tibba_session::{Session, SessionParams, UserSession};
use tibba_util::{JsonParams, JsonResult, sha256, uuid};
use tracing::warn;
use utoipa::ToSchema;
use validator::Validate;

use crate::Result;

/// 本模块日志 target，可用 `RUST_LOG=tibba:router_user=info` 过滤。
const LOG_TARGET: &str = "tibba:router_user";

/// 令牌固定前缀，便于一眼识别 + 中间件快速短路（非本前缀直接跳过 DB 查询）。
const TOKEN_PREFIX: &str = "tibba_";
/// 展示用前缀长度："tibba_" + 8 位 hex。
const PREFIX_DISPLAY_LEN: usize = 14;

/// 生成新令牌，返回 `(明文 token, key_hash, key_prefix)`。
///
/// 明文 = `tibba_` + 64 位随机 hex（两段 uuid 取 sha256）；库中只存 `sha256(明文)`。
fn generate_token() -> (String, String, String) {
    let secret = sha256(format!("{}{}", uuid(), uuid()).as_bytes());
    let token = format!("{TOKEN_PREFIX}{secret}");
    let key_hash = sha256(token.as_bytes());
    let key_prefix: String = token.chars().take(PREFIX_DISPLAY_LEN).collect();
    (token, key_hash, key_prefix)
}

/// 创建 API Key 的请求体。
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub(crate) struct CreateApiKeyBody {
    /// key 标签（区分用途，如 "ci"）
    #[validate(length(min = 1, max = 128))]
    name: String,
    /// 过期天数（1-3650）；省略表示永不过期
    #[validate(range(min = 1, max = 3650))]
    expires_in_days: Option<i32>,
}

/// 创建响应：**`token` 为明文，仅此一次返回，请立即保存。**
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CreateApiKeyResp {
    /// 记录 id（吊销时使用）
    id: i64,
    /// key 标签
    name: String,
    /// 展示用前缀（如 `tibba_a1b2c3d4`）
    key_prefix: String,
    /// 完整令牌明文，**仅此一次返回**
    token: String,
    /// 过期天数（与请求一致；null 表示永不过期）
    expires_in_days: Option<i32>,
}

/// 列表项（不含哈希与明文）。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ApiKeyItem {
    id: i64,
    name: String,
    key_prefix: String,
    last_used_at: Option<String>,
    expires_at: Option<String>,
    created: String,
}

impl From<ApiKey> for ApiKeyItem {
    fn from(k: ApiKey) -> Self {
        Self {
            id: k.id,
            name: k.name,
            key_prefix: k.key_prefix,
            last_used_at: k.last_used_at,
            expires_at: k.expires_at,
            created: k.created,
        }
    }
}

/// `POST /users/api-keys` —— 创建 API Key，返回一次性明文令牌。
#[utoipa::path(
    post,
    path = "/users/api-keys",
    tag = "user",
    request_body = CreateApiKeyBody,
    responses(
        (status = 200, description = "创建成功，token 字段为一次性明文令牌", body = CreateApiKeyResp),
        (status = 401, description = "未登录")
    )
)]
pub(crate) async fn create_api_key(
    State(pool): State<&'static PgPool>,
    session: UserSession,
    JsonParams(body): JsonParams<CreateApiKeyBody>,
) -> JsonResult<CreateApiKeyResp> {
    let (token, key_hash, key_prefix) = generate_token();
    let id = ApiKeyModel::new()
        .create(
            pool,
            CreateApiKeyParams {
                user_id: session.get_user_id(),
                name: &body.name,
                key_prefix: &key_prefix,
                key_hash: &key_hash,
                expires_in_days: body.expires_in_days,
            },
        )
        .await?;
    Ok(Json(CreateApiKeyResp {
        id,
        name: body.name,
        key_prefix,
        token,
        expires_in_days: body.expires_in_days,
    }))
}

/// `GET /users/api-keys` —— 列出当前用户的 API Key（不含哈希/明文）。
#[utoipa::path(
    get,
    path = "/users/api-keys",
    tag = "user",
    responses(
        (status = 200, description = "当前用户的 API Key 列表", body = [ApiKeyItem]),
        (status = 401, description = "未登录")
    )
)]
pub(crate) async fn list_api_keys(
    State(pool): State<&'static PgPool>,
    session: UserSession,
) -> JsonResult<Vec<ApiKeyItem>> {
    let keys = ApiKeyModel::new()
        .list_by_user(pool, session.get_user_id())
        .await?;
    Ok(Json(keys.into_iter().map(ApiKeyItem::from).collect()))
}

/// `DELETE /users/api-keys/{id}` —— 吊销自己的某个 key（幂等：不存在也返回 204）。
#[utoipa::path(
    delete,
    path = "/users/api-keys/{id}",
    tag = "user",
    params(("id" = i64, Path, description = "API Key 记录 id")),
    responses(
        (status = 204, description = "已吊销（幂等）"),
        (status = 401, description = "未登录")
    )
)]
pub(crate) async fn revoke_api_key(
    State(pool): State<&'static PgPool>,
    session: UserSession,
    Path(id): Path<i64>,
) -> Result<StatusCode> {
    // revoke 已带 user_id 归属校验：删别人的 key 影响 0 行，对外同样返回 204（不泄露存在性）
    ApiKeyModel::new()
        .revoke(pool, session.get_user_id(), id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 从请求头提取令牌：优先 `Authorization: Bearer <token>`，回退 `X-API-Key: <token>`。
fn extract_token(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        && let Some(rest) = v.strip_prefix("Bearer ")
    {
        return Some(rest.trim().to_string());
    }
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
}

/// 根据 user_id 装配一个「已登录」Session（account + roles + groups + permissions）。
///
/// `with_account` 会把 iat 置为非 0，故下游 Session 提取器会直接采用本实例而不再读
/// Cookie，从而让 API Key 请求复用既有 `UserSession` / `AdminSession` 鉴权路径。
async fn build_session(
    pool: &'static PgPool,
    cache: &'static RedisCache,
    params: Arc<SessionParams>,
    user_id: i64,
) -> Option<Session> {
    let user = UserModel::new()
        .get_by_id(pool, user_id as u64)
        .await
        .ok()??;
    let roles = user.roles.clone().unwrap_or_default();
    let groups = user.groups.clone().unwrap_or_default();
    let permissions = RolePermissionModel::new()
        .list_permissions_for_roles(pool, &roles)
        .await
        .unwrap_or_default();
    let session = Session::new(cache, params)
        .with_account(&user.account, user.id)
        .with_groups(groups)
        .with_roles(roles)
        .with_permissions(permissions);
    Some(session)
}

/// 全局鉴权中间件：识别 API Key 并注入已登录 Session。
///
/// **不会失败** —— 无令牌 / 非本前缀 / 无效令牌都静默放行（按未登录处理），
/// 真正的 401 由下游 `UserSession` 提取器在受保护路由上给出。须挂在 session
/// 中间件「之后」（更内层），以便用有效 key 的 Session 覆盖空 Session。
pub async fn api_key_auth(
    State((pool, cache, params)): State<(&'static PgPool, &'static RedisCache, Arc<SessionParams>)>,
    mut req: Request,
    next: Next,
) -> Response {
    if let Some(token) = extract_token(req.headers())
        && token.starts_with(TOKEN_PREFIX)
    {
        let key_hash = sha256(token.as_bytes());
        match ApiKeyModel::new()
            .find_active_by_hash(pool, &key_hash)
            .await
        {
            Ok(Some(auth)) => {
                if let Some(session) = build_session(pool, cache, params, auth.user_id).await {
                    req.extensions_mut().insert(session);
                    // 更新最近使用时间，best-effort：失败不影响鉴权
                    if let Err(e) = ApiKeyModel::new().touch_last_used(pool, auth.id).await {
                        warn!(target: LOG_TARGET, error = %e, "touch api key last_used failed");
                    }
                }
            }
            // 无效令牌：不注入，按未登录处理
            Ok(None) => {}
            Err(e) => warn!(target: LOG_TARGET, error = %e, "api key lookup failed"),
        }
    }
    next.run(req).await
}
