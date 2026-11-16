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

//! 应用级签名鉴权（feature = `"app-auth"`）：
//!
//! - 凭据存储：复用 `configurations` 表（固定 `category = "app_client"`、`name = app_id`），
//!   `app_secret` 使用 [`SecretCipher`]（AES-256-GCM + AAD 绑定 `app_id`）加密落库；
//!   面向内部团队服务间鉴权，管理员（`su` / `admin`）可按需解密查询与轮换密钥；
//! - 全局中间件（[`app_auth_middleware`]）：当请求 Query 同时携带 `app_id` 与 `sign` 时，
//!   校验 `ts` 时间窗（`±300s`）及 `sign = sha256(app_secret + ts)`，通过后执行应用级固定窗口限流，
//!   剥离 `app_id` / `ts` / `sign` 三个查询参数，并向请求扩展注入 [`AppClientIdentity`]
//!   （不注入 [`Session`]，应用凭据仅可调用已授权的只读 SQL 查询接口 `{group}/{name}` 或 `{group}/*`）；
//! - 管理端点（`/apps`，`su` / `admin` 可管）：
//!   列表、新建、编辑授权 SQL 查询接口 / 限流 / 时间窗、按需查看密钥、轮换密钥、软删除。

use crate::AppClientIdentity;
use crate::configuration::{APP_CLIENT_CATEGORY, ConfigStore};
use crate::user_agent_of;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use subtle::ConstantTimeEq;
use tibba_auth::{Role, Session, UserSession};
use tibba_cache::RedisCache;
use tibba_crypto::SecretCipher;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_model::{
    AuditLogModel, AuditLogParams, Configuration, ConfigurationModel, Model, Status,
    format_datetime, now_primitive_utc,
};
use tibba_runtime::CTX;
use tibba_util::{JsonParams, JsonResult, sha256, timestamp, uuid};
use tracing::{debug, warn};
use utoipa::{OpenApi, ToSchema};
use validator::Validate;
use zeroize::Zeroizing;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const LOG_TARGET: &str = "tibba:app_auth";
const ERROR_CATEGORY: &str = "app_auth";

const APP_SECRET_CIPHER_DOMAIN: &str = "app_client_secret";
const MAX_APP_ID_LEN: usize = 64;
const SECRET_PREFIX_LEN: usize = 8;
const TIMESTAMP_SKEW_SECS: i64 = 300;
const DEFAULT_RATE_LIMIT_PER_MIN: i64 = 600;
const MAX_RATE_LIMIT_PER_MIN: i64 = 600_000;
const MAX_APP_QUERIES: usize = 256;
const MAX_QUERY_GROUP_LEN: usize = 32;
const MAX_QUERY_NAME_LEN: usize = 64;
const DEFAULT_END_TIME: &str = "2099-12-31T23:59:59Z";
const APP_RATE_WINDOW: Duration = Duration::from_secs(60);

fn app_error(
    sub_category: &str,
    message: impl Into<String>,
    status: u16,
    exception: bool,
) -> BaseError {
    BaseError::new(message.into())
        .with_category(ERROR_CATEGORY)
        .with_sub_category(sub_category)
        .with_status(status)
        .with_exception(exception)
}

fn default_rate_limit_per_min() -> i64 {
    DEFAULT_RATE_LIMIT_PER_MIN
}

fn app_secret_cipher(app_secret: &str) -> SecretCipher {
    SecretCipher::from_app_secret(app_secret, APP_SECRET_CIPHER_DOMAIN)
}

fn app_aad(app_id: &str) -> String {
    format!("app_client:{app_id}")
}

fn default_start_time() -> String {
    format_datetime(now_primitive_utc())
}

fn is_valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_APP_ID_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn validate_app_id(app_id: &str, status_on_error: u16) -> Result<()> {
    if !is_valid_app_id(app_id) {
        let sub_category = if status_on_error == 401 {
            "invalid_signature"
        } else {
            "invalid_param"
        };
        return Err(app_error(
            sub_category,
            "invalid app_id (1..=64 ASCII letters, digits, '_' or '-')",
            status_on_error,
            false,
        ));
    }
    Ok(())
}

fn validate_rate_limit(limit: i64) -> Result<()> {
    if !(0..=MAX_RATE_LIMIT_PER_MIN).contains(&limit) {
        return Err(app_error(
            "invalid_param",
            format!("rate_limit_per_min must be between 0 and {MAX_RATE_LIMIT_PER_MIN}"),
            400,
            false,
        ));
    }
    Ok(())
}

fn generate_app_secret() -> (Zeroizing<String>, String) {
    let secret = Zeroizing::new(sha256(format!("{}{}", uuid(), uuid()).as_bytes()));
    let prefix: String = secret.chars().take(SECRET_PREFIX_LEN).collect();
    (secret, prefix)
}

/// 生成应用签名：`sha256("{app_secret}{ts}")`。
#[must_use]
pub fn compute_app_sign(app_secret: &str, ts: i64) -> String {
    sha256(format!("{app_secret}{ts}").as_bytes())
}

/// 校验时间戳偏差（`±300s`）并常数时间比对 64 位十六进制签名。
pub(crate) fn verify_app_sign(app_secret: &str, ts: i64, now_ts: i64, sign: &str) -> Result<()> {
    let within_window = now_ts
        .checked_sub(ts)
        .and_then(i64::checked_abs)
        .is_some_and(|diff| diff <= TIMESTAMP_SKEW_SECS);
    if !within_window {
        return Err(app_error(
            "signature_expired",
            "app signature timestamp expired",
            401,
            false,
        ));
    }
    if sign.len() != 64 || !sign.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(app_error(
            "invalid_signature",
            "invalid app signature format",
            401,
            false,
        ));
    }
    let expected = compute_app_sign(app_secret, ts);
    let normalized = sign.to_ascii_lowercase();
    if expected.len() != normalized.len()
        || !bool::from(expected.as_bytes().ct_eq(normalized.as_bytes()))
    {
        return Err(app_error(
            "invalid_signature",
            "app signature mismatch",
            401,
            false,
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct AppAuthQuery {
    pub app_id: String,
    pub ts: i64,
    pub sign: String,
    pub remaining_query: String,
}

/// 解析查询串；仅当 `app_id` 与 `sign` 均存在时触发应用鉴权并剥离 `app_id`/`ts`/`sign`。
pub(crate) fn extract_and_strip_auth_query(
    raw_query: Option<&str>,
) -> Option<Result<AppAuthQuery>> {
    let query = raw_query?;
    if query.is_empty() {
        return None;
    }

    let mut app_id: Option<String> = None;
    let mut ts_raw: Option<&str> = None;
    let mut sign: Option<String> = None;
    let mut remaining = Vec::new();

    for seg in query.split('&') {
        if seg.is_empty() {
            continue;
        }
        let (key, val) = seg.split_once('=').unwrap_or((seg, ""));
        match key {
            "app_id" => app_id = Some(val.to_string()),
            "ts" => ts_raw = Some(val),
            "sign" => sign = Some(val.to_string()),
            _ => remaining.push(seg),
        }
    }

    let (Some(app_id), Some(sign)) = (app_id, sign) else {
        return None;
    };

    if app_id.is_empty() {
        return Some(Err(app_error(
            "invalid_signature",
            "missing app_id",
            401,
            false,
        )));
    }
    let Some(ts_str) = ts_raw.filter(|s| !s.is_empty()) else {
        return Some(Err(app_error(
            "invalid_signature",
            "missing ts",
            401,
            false,
        )));
    };
    let ts = match ts_str.parse::<i64>() {
        Ok(v) => v,
        Err(_) => {
            return Some(Err(app_error(
                "invalid_signature",
                "invalid ts timestamp",
                401,
                false,
            )));
        }
    };

    Some(Ok(AppAuthQuery {
        app_id,
        ts,
        sign,
        remaining_query: remaining.join("&"),
    }))
}

fn strip_uri_auth_query(uri: &Uri, remaining_query: &str) -> Result<Uri> {
    let mut parts = uri.clone().into_parts();
    let path = uri.path();
    let new_pq = if remaining_query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{remaining_query}")
    };
    parts.path_and_query = Some(
        new_pq
            .parse()
            .map_err(|_| app_error("invalid_signature", "failed to rewrite query", 400, false))?,
    );
    Uri::from_parts(parts)
        .map_err(|_| app_error("invalid_signature", "failed to rebuild uri", 400, false))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAppClientData {
    secret_ciphertext: String,
    secret_prefix: String,
    #[serde(default)]
    queries: Vec<String>,
    #[serde(default = "default_rate_limit_per_min")]
    rate_limit_per_min: i64,
}

fn decrypt_app_secret(
    cipher: &SecretCipher,
    app_id: &str,
    data: Value,
) -> Result<(StoredAppClientData, Zeroizing<String>)> {
    let stored: StoredAppClientData = serde_json::from_value(data).map_err(|e| {
        app_error(
            "stored_json",
            format!("invalid stored app_client data: {e}"),
            500,
            true,
        )
    })?;
    let plain_bytes = Zeroizing::new(
        cipher
            .decrypt_with_aad(&stored.secret_ciphertext, app_aad(app_id).as_bytes())
            .inspect_err(
                |e| warn!(target: LOG_TARGET, app_id, error = %e, "decrypt app_client secret failed"),
            )?,
    );
    let plain_str = String::from_utf8(plain_bytes.to_vec())
        .map_err(|_| app_error("stored_json", "corrupted app secret encoding", 500, true))?;
    Ok((stored, Zeroizing::new(plain_str)))
}

/// [`app_auth_middleware`] 运行所需状态。
#[derive(Clone)]
pub struct AppAuthState {
    configs: &'static ConfigStore,
    cache: &'static RedisCache,
    cipher: Arc<SecretCipher>,
}

impl fmt::Debug for AppAuthState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppAuthState")
            .field("cipher", &self.cipher)
            .finish_non_exhaustive()
    }
}

/// 构造应用签名鉴权中间件状态。
#[must_use]
pub fn new_app_auth_state(
    configs: &'static ConfigStore,
    cache: &'static RedisCache,
    app_secret: &str,
) -> AppAuthState {
    AppAuthState {
        configs,
        cache,
        cipher: Arc::new(app_secret_cipher(app_secret)),
    }
}

async fn app_rate_limited(cache: &RedisCache, app_id: &str, limit: i64) -> Option<Response> {
    if limit <= 0 {
        return None;
    }
    let redis_key = format!("app_rate:{app_id}");
    match cache.incr(&redis_key, 1, Some(APP_RATE_WINDOW)).await {
        Ok(count) if count > limit => {
            warn!(target: LOG_TARGET, app_id, count, limit, "app rate limit exceeded");
            Some(app_error("rate_limited", "app rate limit exceeded", 429, false).into_response())
        }
        Ok(_) => None,
        Err(e) => {
            warn!(target: LOG_TARGET, app_id, error = %e, "app rate limit check failed, allow");
            None
        }
    }
}

/// 全局应用签名鉴权中间件（须挂在 session 中间件内侧、CSRF 中间件外侧）。
pub async fn app_auth_middleware(
    State(state): State<AppAuthState>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(parsed) = extract_and_strip_auth_query(req.uri().query()) else {
        return next.run(req).await;
    };

    let AppAuthQuery {
        app_id,
        ts,
        sign,
        remaining_query,
    } = match parsed {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };

    if let Err(e) = validate_app_id(&app_id, 401) {
        return e.into_response();
    }

    let cfg = match state
        .configs
        .get_effective(APP_CLIENT_CATEGORY, &app_id)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return app_error(
                "app_not_found",
                "app client not found or inactive",
                401,
                false,
            )
            .into_response();
        }
        Err(e) => return e.into_response(),
    };

    let (stored, plain_str) = match decrypt_app_secret(&state.cipher, &app_id, cfg.data) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };

    if let Err(e) = verify_app_sign(&plain_str, ts, timestamp(), &sign) {
        return e.into_response();
    }

    if let Some(resp) = app_rate_limited(state.cache, &app_id, stored.rate_limit_per_min).await {
        return resp;
    }

    let cleaned_uri = match strip_uri_auth_query(req.uri(), &remaining_query) {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    *req.uri_mut() = cleaned_uri;

    let account = format!("app:{app_id}");
    debug!(target: LOG_TARGET, app_id, "authenticated app client request");
    req.extensions_mut().insert(AppClientIdentity {
        app_id,
        allowed_queries: stored.queries,
    });
    let _ = CTX.try_with(|ctx| ctx.set_account(&account));

    next.run(req).await
}

#[derive(Clone)]
struct AppAdminState {
    pool: &'static PgPool,
    configs: &'static ConfigStore,
    cipher: Arc<SecretCipher>,
}

fn ensure_admin_caller(session: &Session) -> Result<()> {
    if !session.is_login() {
        return Err(app_error("not_logged_in", "login required", 401, false));
    }
    let is_admin = session
        .get_roles()
        .iter()
        .any(|r| matches!(Role::from(r.as_str()), Role::SuperAdmin | Role::Admin));
    if !is_admin {
        return Err(app_error(
            "not_admin",
            "admin or super admin required",
            403,
            false,
        ));
    }
    Ok(())
}

fn is_valid_query_segment(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// 校验并规范化应用可调用的 SQL 查询接口列表（仅允许 `{group}/{name}` 或 `{group}/*`）。
pub(crate) fn validate_app_queries(queries: &[String]) -> Result<Vec<String>> {
    if queries.len() > MAX_APP_QUERIES {
        return Err(app_error(
            "invalid_param",
            format!("at most {MAX_APP_QUERIES} query grants can be configured"),
            400,
            false,
        ));
    }
    let mut normalized = BTreeSet::new();
    for raw in queries {
        let item = raw.trim();
        let valid = match item.split_once('/') {
            Some((group, name)) => {
                is_valid_query_segment(group, MAX_QUERY_GROUP_LEN)
                    && (name == "*" || is_valid_query_segment(name, MAX_QUERY_NAME_LEN))
            }
            None => false,
        };
        if !valid {
            return Err(app_error(
                "invalid_param",
                format!(
                    "invalid query grant '{item}' (expected '{{group}}/{{name}}' or '{{group}}/*')"
                ),
                400,
                false,
            ));
        }
        normalized.insert(item.to_string());
    }
    Ok(normalized.into_iter().collect())
}

/// 管理端展示的应用授权条目（绝不暴露密钥明文或密文）。
#[derive(Debug, Serialize, ToSchema)]
pub struct AppClientAdminItem {
    pub id: i64,
    pub status: i16,
    pub app_id: String,
    pub description: String,
    pub secret_prefix: String,
    pub queries: Vec<String>,
    pub rate_limit_per_min: i64,
    pub effective_start_time: String,
    pub effective_end_time: String,
    pub created: String,
    pub modified: String,
}

impl From<Configuration> for AppClientAdminItem {
    fn from(cfg: Configuration) -> Self {
        let stored = serde_json::from_value::<StoredAppClientData>(cfg.data).ok();
        let (secret_prefix, queries, rate_limit_per_min) = match stored {
            Some(s) => (s.secret_prefix, s.queries, s.rate_limit_per_min),
            None => (String::new(), Vec::new(), DEFAULT_RATE_LIMIT_PER_MIN),
        };
        Self {
            id: cfg.id,
            status: cfg.status,
            app_id: cfg.name,
            description: cfg.description,
            secret_prefix,
            queries,
            rate_limit_per_min,
            effective_start_time: cfg.effective_start_time,
            effective_end_time: cfg.effective_end_time,
            created: cfg.created,
            modified: cfg.modified,
        }
    }
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
struct CreateAppBody {
    #[validate(length(min = 1, max = 64))]
    app_id: String,
    #[serde(default)]
    #[validate(length(max = 255))]
    description: String,
    #[serde(default)]
    queries: Vec<String>,
    rate_limit_per_min: Option<i64>,
    status: Option<i16>,
    effective_start_time: Option<String>,
    effective_end_time: Option<String>,
}

/// 创建应用响应（含 `app_secret` 明文）。
#[derive(Debug, Serialize, ToSchema)]
struct CreateAppResp {
    id: u64,
    app_id: String,
    app_secret: String,
    secret_prefix: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
struct UpdateAppBody {
    #[validate(length(max = 255))]
    description: Option<String>,
    status: Option<i16>,
    queries: Option<Vec<String>>,
    rate_limit_per_min: Option<i64>,
    effective_start_time: Option<String>,
    effective_end_time: Option<String>,
}

/// 应用密钥读取 / 轮换响应。
#[derive(Debug, Serialize, ToSchema)]
struct AppSecretResp {
    app_id: String,
    app_secret: String,
    secret_prefix: String,
}

#[allow(clippy::too_many_arguments)]
async fn log_app_audit(
    pool: &PgPool,
    session: &Session,
    action: &str,
    target_id: &str,
    request_id: &RequestId,
    ip: std::net::IpAddr,
    headers: &HeaderMap,
    detail: Value,
) {
    let _ = AuditLogModel::new()
        .log(
            pool,
            AuditLogParams::new(action)
                .with_user(session.get_user_id())
                .with_target("app_client", target_id)
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(headers))
                .with_detail(detail),
        )
        .await;
}

/// 列出全部授权应用（`su` / `admin`）。
#[utoipa::path(
    get,
    path = "/apps",
    tag = "app_auth",
    responses(
        (status = 200, description = "授权应用列表", body = Vec<AppClientAdminItem>),
        (status = 403, description = "需要管理员或超级管理员权限")
    )
)]
async fn list_apps(
    State(state): State<AppAdminState>,
    session: UserSession,
) -> JsonResult<Vec<AppClientAdminItem>> {
    ensure_admin_caller(&session)?;
    let rows = ConfigurationModel::new()
        .list_by_category(state.pool, APP_CLIENT_CATEGORY)
        .await?;
    Ok(Json(
        rows.into_iter().map(AppClientAdminItem::from).collect(),
    ))
}

/// 创建授权应用并签发一次性明文 `app_secret`（`su` / `admin`）。
#[utoipa::path(
    post,
    path = "/apps",
    tag = "app_auth",
    request_body = CreateAppBody,
    responses(
        (status = 200, description = "创建成功（含 app_secret 明文）", body = CreateAppResp),
        (status = 400, description = "参数或查询接口授权规则非法"),
        (status = 403, description = "需要管理员或超级管理员权限"),
        (status = 409, description = "app_id 已存在")
    )
)]
async fn create_app(
    State(state): State<AppAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(body): JsonParams<CreateAppBody>,
) -> JsonResult<CreateAppResp> {
    ensure_admin_caller(&session)?;
    validate_app_id(&body.app_id, 400)?;
    let queries = validate_app_queries(&body.queries)?;
    let rate_limit = body
        .rate_limit_per_min
        .unwrap_or(DEFAULT_RATE_LIMIT_PER_MIN);
    validate_rate_limit(rate_limit)?;

    let model = ConfigurationModel::new();
    if model
        .get_by_category_name(state.pool, APP_CLIENT_CATEGORY, &body.app_id)
        .await?
        .is_some()
    {
        return Err(app_error(
            "app_exists",
            format!("app_id '{}' already exists", body.app_id),
            409,
            false,
        ));
    }

    let (secret_plain, secret_prefix) = generate_app_secret();
    let secret_ciphertext = state
        .cipher
        .encrypt_with_aad(secret_plain.as_bytes(), app_aad(&body.app_id).as_bytes())?;

    let stored = StoredAppClientData {
        secret_ciphertext,
        secret_prefix: secret_prefix.clone(),
        queries: queries.clone(),
        rate_limit_per_min: rate_limit,
    };
    let data = serde_json::to_value(&stored).map_err(|e| {
        app_error(
            "stored_json",
            format!("serialize app_client failed: {e}"),
            500,
            true,
        )
    })?;

    let start_str = body
        .effective_start_time
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(default_start_time);
    let end_str = body
        .effective_end_time
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| DEFAULT_END_TIME.to_string());

    let insert_payload = json!({
        "category": APP_CLIENT_CATEGORY,
        "name": &body.app_id,
        "data": data,
        "description": body.description,
        "status": body.status.unwrap_or(Status::Enabled as i16),
        "effective_start_time": start_str,
        "effective_end_time": end_str,
    });

    let id = model.insert(state.pool, insert_payload).await?;
    state
        .configs
        .invalidate(APP_CLIENT_CATEGORY, &body.app_id)
        .await;

    log_app_audit(
        state.pool,
        &session,
        "app_client.create",
        &body.app_id,
        &request_id,
        ip,
        &headers,
        json!({ "id": id, "queries": queries, "rate_limit_per_min": rate_limit }),
    )
    .await;

    Ok(Json(CreateAppResp {
        id,
        app_id: body.app_id,
        app_secret: secret_plain.to_string(),
        secret_prefix,
    }))
}

/// 更新授权应用的 SQL 查询接口白名单、限流、状态或生效时间窗（保留原密文，`su` / `admin`）。
#[utoipa::path(
    patch,
    path = "/apps/{id}",
    tag = "app_auth",
    params(("id" = u64, Path, description = "configurations 主键 ID")),
    request_body = UpdateAppBody,
    responses(
        (status = 204, description = "更新成功"),
        (status = 400, description = "参数或查询接口授权规则非法"),
        (status = 403, description = "需要管理员或超级管理员权限"),
        (status = 404, description = "应用记录不存在")
    )
)]
async fn update_app(
    State(state): State<AppAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(id): Path<u64>,
    JsonParams(body): JsonParams<UpdateAppBody>,
) -> Result<StatusCode> {
    ensure_admin_caller(&session)?;
    let model = ConfigurationModel::new();
    let Some(existing) = model.get_by_id(state.pool, id).await? else {
        return Err(app_error(
            "app_not_found",
            "app client not found",
            404,
            false,
        ));
    };
    if existing.category != APP_CLIENT_CATEGORY {
        return Err(app_error(
            "app_not_found",
            "configuration is not an app_client item",
            404,
            false,
        ));
    }

    let mut stored: StoredAppClientData = serde_json::from_value(existing.data).map_err(|e| {
        app_error(
            "stored_json",
            format!("invalid stored app_client data: {e}"),
            500,
            true,
        )
    })?;

    let mut data_changed = false;
    if let Some(queries) = body.queries.as_ref() {
        let validated = validate_app_queries(queries)?;
        stored.queries = validated;
        data_changed = true;
    }
    if let Some(limit) = body.rate_limit_per_min {
        validate_rate_limit(limit)?;
        stored.rate_limit_per_min = limit;
        data_changed = true;
    }

    let mut update_map = serde_json::Map::new();
    if data_changed {
        let data_val = serde_json::to_value(&stored).map_err(|e| {
            app_error(
                "stored_json",
                format!("serialize app_client failed: {e}"),
                500,
                true,
            )
        })?;
        update_map.insert("data".to_string(), data_val);
    }
    if let Some(desc) = body.description {
        update_map.insert("description".to_string(), Value::String(desc));
    }
    if let Some(status) = body.status {
        update_map.insert("status".to_string(), json!(status));
    }
    if let Some(start) = body.effective_start_time.filter(|s| !s.trim().is_empty()) {
        update_map.insert("effective_start_time".to_string(), Value::String(start));
    }
    if let Some(end) = body.effective_end_time.filter(|s| !s.trim().is_empty()) {
        update_map.insert("effective_end_time".to_string(), Value::String(end));
    }

    model
        .update_by_id(state.pool, id, Value::Object(update_map))
        .await?;
    state
        .configs
        .invalidate(APP_CLIENT_CATEGORY, &existing.name)
        .await;

    log_app_audit(
        state.pool,
        &session,
        "app_client.update",
        &existing.name,
        &request_id,
        ip,
        &headers,
        json!({ "id": id }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 按需解密并查看应用密钥明文（仅限 `su` / `admin`，操作计入审计日志）。
#[utoipa::path(
    get,
    path = "/apps/{id}/secret",
    tag = "app_auth",
    params(("id" = u64, Path, description = "configurations 主键 ID")),
    responses(
        (status = 200, description = "解密后的 app_secret 明文", body = AppSecretResp),
        (status = 403, description = "需要管理员或超级管理员权限"),
        (status = 404, description = "应用记录不存在")
    )
)]
async fn get_app_secret(
    State(state): State<AppAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> JsonResult<AppSecretResp> {
    ensure_admin_caller(&session)?;
    let Some(existing) = ConfigurationModel::new().get_by_id(state.pool, id).await? else {
        return Err(app_error(
            "app_not_found",
            "app client not found",
            404,
            false,
        ));
    };
    if existing.category != APP_CLIENT_CATEGORY {
        return Err(app_error(
            "app_not_found",
            "configuration is not an app_client item",
            404,
            false,
        ));
    }

    let (stored, plain_secret) = decrypt_app_secret(&state.cipher, &existing.name, existing.data)?;

    log_app_audit(
        state.pool,
        &session,
        "app_client.view_secret",
        &existing.name,
        &request_id,
        ip,
        &headers,
        json!({ "id": id, "secret_prefix": stored.secret_prefix }),
    )
    .await;

    Ok(Json(AppSecretResp {
        app_id: existing.name,
        app_secret: plain_secret.to_string(),
        secret_prefix: stored.secret_prefix,
    }))
}

/// 轮换应用密钥并返回新 `app_secret` 明文（`su` / `admin`，旧密钥立即失效）。
#[utoipa::path(
    post,
    path = "/apps/{id}/rotate-secret",
    tag = "app_auth",
    params(("id" = u64, Path, description = "configurations 主键 ID")),
    responses(
        (status = 200, description = "轮换成功（返回新 app_secret 明文）", body = AppSecretResp),
        (status = 403, description = "需要管理员或超级管理员权限"),
        (status = 404, description = "应用记录不存在")
    )
)]
async fn rotate_app_secret(
    State(state): State<AppAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> JsonResult<AppSecretResp> {
    ensure_admin_caller(&session)?;
    let model = ConfigurationModel::new();
    let Some(existing) = model.get_by_id(state.pool, id).await? else {
        return Err(app_error(
            "app_not_found",
            "app client not found",
            404,
            false,
        ));
    };
    if existing.category != APP_CLIENT_CATEGORY {
        return Err(app_error(
            "app_not_found",
            "configuration is not an app_client item",
            404,
            false,
        ));
    }

    let mut stored: StoredAppClientData = serde_json::from_value(existing.data).map_err(|e| {
        app_error(
            "stored_json",
            format!("invalid stored app_client data: {e}"),
            500,
            true,
        )
    })?;

    let (secret_plain, secret_prefix) = generate_app_secret();
    stored.secret_ciphertext = state
        .cipher
        .encrypt_with_aad(secret_plain.as_bytes(), app_aad(&existing.name).as_bytes())?;
    stored.secret_prefix = secret_prefix.clone();

    let data_val = serde_json::to_value(&stored).map_err(|e| {
        app_error(
            "stored_json",
            format!("serialize app_client failed: {e}"),
            500,
            true,
        )
    })?;

    model
        .update_by_id(state.pool, id, json!({ "data": data_val }))
        .await?;
    state
        .configs
        .invalidate(APP_CLIENT_CATEGORY, &existing.name)
        .await;

    log_app_audit(
        state.pool,
        &session,
        "app_client.rotate_secret",
        &existing.name,
        &request_id,
        ip,
        &headers,
        json!({ "id": id, "secret_prefix": secret_prefix }),
    )
    .await;

    Ok(Json(AppSecretResp {
        app_id: existing.name,
        app_secret: secret_plain.to_string(),
        secret_prefix,
    }))
}

/// 软删除授权应用（`su` / `admin`）。
#[utoipa::path(
    delete,
    path = "/apps/{id}",
    tag = "app_auth",
    params(("id" = u64, Path, description = "configurations 主键 ID")),
    responses(
        (status = 204, description = "删除成功"),
        (status = 403, description = "需要管理员或超级管理员权限"),
        (status = 404, description = "应用记录不存在")
    )
)]
async fn delete_app(
    State(state): State<AppAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Result<StatusCode> {
    ensure_admin_caller(&session)?;
    let model = ConfigurationModel::new();
    let Some(existing) = model.get_by_id(state.pool, id).await? else {
        return Err(app_error(
            "app_not_found",
            "app client not found",
            404,
            false,
        ));
    };
    if existing.category != APP_CLIENT_CATEGORY {
        return Err(app_error(
            "app_not_found",
            "configuration is not an app_client item",
            404,
            false,
        ));
    }

    model.delete_by_id(state.pool, id).await?;
    state
        .configs
        .invalidate(APP_CLIENT_CATEGORY, &existing.name)
        .await;

    log_app_audit(
        state.pool,
        &session,
        "app_client.delete",
        &existing.name,
        &request_id,
        ip,
        &headers,
        json!({ "id": id }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 构建应用授权管理端路由（挂载于 `/apps`）。
pub fn new_app_admin_router(
    pool: &'static PgPool,
    configs: &'static ConfigStore,
    app_secret: &str,
) -> Router {
    let state = AppAdminState {
        pool,
        configs,
        cipher: Arc::new(app_secret_cipher(app_secret)),
    };
    Router::new()
        .route("/", get(list_apps).post(create_app))
        .route("/{id}", patch(update_app).delete(delete_app))
        .route("/{id}/secret", get(get_app_secret))
        .route("/{id}/rotate-secret", post(rotate_app_secret))
        .with_state(state)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_apps,
        create_app,
        update_app,
        get_app_secret,
        rotate_app_secret,
        delete_app
    ),
    components(schemas(
        AppClientAdminItem,
        CreateAppBody,
        CreateAppResp,
        UpdateAppBody,
        AppSecretResp,
    )),
    tags((name = "app_auth", description = "团队内部服务间签名鉴权管理（app_id + app_secret + ts）"))
)]
struct AppAuthApiDoc;

/// 返回 `app_auth` 管理路由的 OpenAPI 文档片段。
pub fn openapi() -> utoipa::openapi::OpenApi {
    AppAuthApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_app_sign_checks_window_format_and_hmac_digest() {
        let secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let now = 1_700_000_000_i64;
        let ts = now - 60;
        let sign = compute_app_sign(secret, ts);
        assert_eq!(sign.len(), 64);

        assert!(verify_app_sign(secret, ts, now, &sign).is_ok());
        assert!(verify_app_sign(secret, ts, now, &sign.to_ascii_uppercase()).is_ok());

        // 超出 ±300s 窗口与极端时间戳（防止溢Panic）均返回 401 signature_expired
        for bad_ts in [now - 301, now + 301, i64::MIN, i64::MAX] {
            let err = verify_app_sign(secret, bad_ts, now, &sign).expect_err("expired ts");
            assert_eq!(err.status(), 401);
            assert_eq!(err.sub_category(), Some("signature_expired"));
        }

        // 格式或签名不匹配返回 401 invalid_signature
        let err_len = verify_app_sign(secret, ts, now, "short").expect_err("short sign");
        assert_eq!(err_len.status(), 401);
        assert_eq!(err_len.sub_category(), Some("invalid_signature"));

        let bad_sign = "f".repeat(64);
        let err_mismatch = verify_app_sign(secret, ts, now, &bad_sign).expect_err("mismatch");
        assert_eq!(err_mismatch.status(), 401);
        assert_eq!(err_mismatch.sub_category(), Some("invalid_signature"));
    }

    #[test]
    fn extract_and_strip_auth_query_preserves_business_params() {
        assert!(extract_and_strip_auth_query(None).is_none());
        assert!(extract_and_strip_auth_query(Some("page=1&app_id=only")).is_none());

        let auth_query = extract_and_strip_auth_query(Some(
            "limit=20&app_id=billing_svc&ts=1700000000&sign=abc&status=active",
        ))
        .expect("triggered app_auth")
        .expect("valid auth query");
        assert_eq!(auth_query.app_id, "billing_svc");
        assert_eq!(auth_query.ts, 1_700_000_000);
        assert_eq!(auth_query.sign, "abc");
        assert_eq!(auth_query.remaining_query, "limit=20&status=active");

        let uri: Uri = "/api/queries/report/orders?limit=20&app_id=billing_svc&ts=1700000000&sign=abc&status=active"
            .parse()
            .expect("uri parse");
        let cleaned = strip_uri_auth_query(&uri, &auth_query.remaining_query).expect("strip query");
        assert_eq!(
            cleaned.to_string(),
            "/api/queries/report/orders?limit=20&status=active"
        );

        let full_uri: Uri = "https://example.com/api/queries/report/orders?app_id=a&ts=1&sign=b"
            .parse()
            .expect("full uri parse");
        let cleaned_full = strip_uri_auth_query(&full_uri, "").expect("strip empty query");
        assert_eq!(
            cleaned_full.to_string(),
            "https://example.com/api/queries/report/orders"
        );
    }

    #[test]
    fn secret_cipher_aad_isolation_binds_app_id() {
        let cipher = app_secret_cipher("master-secret-value");
        let encrypted = cipher
            .encrypt_with_aad(b"my-app-secret", app_aad("svc_a").as_bytes())
            .expect("encrypt");
        assert_eq!(
            cipher
                .decrypt_with_aad(&encrypted, app_aad("svc_a").as_bytes())
                .expect("decrypt own"),
            b"my-app-secret"
        );
        assert!(
            cipher
                .decrypt_with_aad(&encrypted, app_aad("svc_b").as_bytes())
                .is_err()
        );
    }

    #[test]
    fn validate_app_queries_accepts_exact_and_group_wildcard_only() {
        let queries = validate_app_queries(&[
            " report/orders ".to_string(),
            "group1/*".to_string(),
            "report/orders".to_string(),
            "order-stats_01/daily_summary".to_string(),
        ])
        .expect("valid exact and group wildcard queries allowed");
        assert_eq!(
            queries,
            vec![
                "group1/*".to_string(),
                "order-stats_01/daily_summary".to_string(),
                "report/orders".to_string(),
            ]
        );

        let too_long_group = format!("{}/q1", "g".repeat(MAX_QUERY_GROUP_LEN + 1));
        let too_long_name = format!("g/{}", "n".repeat(MAX_QUERY_NAME_LEN + 1));
        for bad in [
            "*",
            "*/*",
            "*/orders",
            "group1/",
            "/orders",
            "group1/a/b",
            "group1/na*me",
            "group1",
            "",
            &too_long_group,
            &too_long_name,
        ] {
            let err = validate_app_queries(&[bad.to_string()])
                .expect_err("invalid query grant must fail");
            assert_eq!(err.status(), 400);
            assert_eq!(err.sub_category(), Some("invalid_param"));
        }

        // 兼容历史库中仍包含 permissions 字段的配置行反序列化
        let legacy: StoredAppClientData = serde_json::from_value(json!({
            "secret_ciphertext": "enc",
            "secret_prefix": "f49022c2",
            "permissions": ["model:configuration:read"],
            "rate_limit_per_min": 600
        }))
        .expect("legacy permissions payload must deserialize");
        assert!(legacy.queries.is_empty());
        assert_eq!(legacy.secret_prefix, "f49022c2");
    }
}
