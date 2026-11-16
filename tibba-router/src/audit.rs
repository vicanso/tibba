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

//! 系统审计日志查询路由（挂载于 `/audit-logs`）：
//!
//! - `GET /audit-logs`                    —— 多维分页筛选查询（需 Admin 或 `audit:read`）
//! - `GET /audit-logs/trace/{request_id}` —— 按请求 ID 串联完整审计链路（需 Admin 或 `audit:read`）

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tibba_auth::{Session, UserSession, permission_grants};
use tibba_error::Error as BaseError;
use tibba_model::{AuditLog, AuditLogListParams, AuditLogModel, parse_primitive_datetime};
use tibba_util::{JsonResult, QueryParams};
use time::PrimitiveDateTime;
use utoipa::{IntoParams, OpenApi, ToSchema};
use validator::Validate;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "audit";
const MAX_REQUEST_ID_LEN: usize = 128;

fn default_page() -> u64 {
    1
}

fn default_limit() -> u64 {
    50
}

fn audit_error(sub_category: &str, message: impl Into<String>, status: u16) -> BaseError {
    BaseError::new(message.into())
        .with_category(ERROR_CATEGORY)
        .with_sub_category(sub_category)
        .with_status(status)
        .with_exception(false)
}

/// 判断调用方是否有权读取系统审计日志。
pub(crate) fn can_read_audit(is_admin: bool, permissions: &[String]) -> bool {
    is_admin || permission_grants(permissions, "audit:read")
}

fn ensure_can_read_audit(session: &Session) -> Result<()> {
    if can_read_audit(session.is_admin(), session.get_permissions()) {
        Ok(())
    } else {
        Err(audit_error(
            "permission_denied",
            "audit:read permission required",
            403,
        ))
    }
}

fn validate_request_id(request_id: &str) -> Result<()> {
    if request_id.is_empty() || request_id.len() > MAX_REQUEST_ID_LEN {
        return Err(audit_error(
            "invalid_request_id",
            "invalid request_id parameter",
            400,
        ));
    }
    Ok(())
}

fn parse_optional_time(raw: Option<String>) -> Result<Option<PrimitiveDateTime>> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(parse_primitive_datetime(s)?)),
        None => Ok(None),
    }
}

/// 对外暴露的单条审计日志条目（复用至管理员审计检索与用户个人安全活动）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuditLogItem {
    /// 日志主键 ID
    pub id: i64,
    /// 操作人用户 ID（匿名或系统事件时为 `null`）
    pub user_id: Option<i64>,
    /// 动作标识（如 `user.login` / `rbac.role_grant`）
    pub action: String,
    /// 目标资源类型（如 `user` / `role` / `permission`）
    pub target_type: String,
    /// 目标资源标识
    pub target_id: String,
    /// 结构化补充详情 JSON
    #[schema(value_type = Object)]
    pub detail: serde_json::Value,
    /// 关联请求 ID
    pub request_id: String,
    /// 客户端 IP
    pub ip: String,
    /// 客户端 User-Agent
    pub user_agent: String,
    /// 记录创建时间（RFC3339）
    pub created: String,
}

impl From<AuditLog> for AuditLogItem {
    fn from(log: AuditLog) -> Self {
        Self {
            id: log.id,
            user_id: log.user_id,
            action: log.action,
            target_type: log.target_type,
            target_id: log.target_id,
            detail: log.detail,
            request_id: log.request_id,
            ip: log.ip,
            user_agent: log.user_agent,
            created: log.created,
        }
    }
}

/// 审计日志分页查询响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct AuditLogListResp {
    /// 总条数（`count=false` 时为 `-1`）
    pub count: i64,
    /// 本页审计记录
    pub audit_logs: Vec<AuditLogItem>,
}

/// 审计日志多维筛选参数。
#[derive(Debug, Deserialize, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct AuditLogQuery {
    /// 页码（从 1 起，默认 1）
    #[serde(default = "default_page")]
    #[validate(range(min = 1, max = 100_000))]
    page: u64,
    /// 每页条数（默认 50，最大 200）
    #[serde(default = "default_limit")]
    #[validate(range(min = 1, max = 200))]
    limit: u64,
    /// 是否计算总条数
    #[serde(default)]
    count: bool,
    /// 按用户 ID 过滤
    user_id: Option<i64>,
    /// 按精确 action 过滤
    #[validate(length(max = 64))]
    action: Option<String>,
    /// 按 target_type 过滤
    #[validate(length(max = 64))]
    target_type: Option<String>,
    /// 按 target_id 过滤
    #[validate(length(max = 64))]
    target_id: Option<String>,
    /// 按 request_id 过滤
    #[validate(length(max = 128))]
    request_id: Option<String>,
    /// 按 IP 过滤
    #[validate(length(max = 64))]
    ip: Option<String>,
    /// 起始时间（RFC3339 或 `YYYY-MM-DD HH:MM:SS`）
    #[validate(length(max = 64))]
    start_time: Option<String>,
    /// 截止时间（RFC3339 或 `YYYY-MM-DD HH:MM:SS`）
    #[validate(length(max = 64))]
    end_time: Option<String>,
}

/// 多维分页检索系统审计日志。
#[utoipa::path(
    get,
    path = "/audit-logs",
    tag = "audit",
    params(AuditLogQuery),
    responses(
        (status = 200, description = "分页审计日志列表", body = AuditLogListResp),
        (status = 400, description = "查询参数或时间格式非法"),
        (status = 403, description = "缺少 audit:read 权限")
    )
)]
async fn list_audit_logs(
    State(pool): State<&'static PgPool>,
    session: UserSession,
    QueryParams(query): QueryParams<AuditLogQuery>,
) -> JsonResult<AuditLogListResp> {
    ensure_can_read_audit(&session)?;

    let list_params = AuditLogListParams {
        page: query.page,
        limit: query.limit,
        user_id: query.user_id,
        action: query.action,
        target_type: query.target_type,
        target_id: query.target_id,
        request_id: query.request_id,
        ip: query.ip,
        start_time: parse_optional_time(query.start_time)?,
        end_time: parse_optional_time(query.end_time)?,
    };

    let (rows, total) = AuditLogModel::new()
        .list_and_count(pool, query.count, &list_params)
        .await?;
    let audit_logs = rows.into_iter().map(AuditLogItem::from).collect();

    Ok(Json(AuditLogListResp {
        count: total,
        audit_logs,
    }))
}

/// 按 `request_id` 串联同一次请求触发的所有审计事件（按创建时间升序）。
#[utoipa::path(
    get,
    path = "/audit-logs/trace/{request_id}",
    tag = "audit",
    params(("request_id" = String, Path, description = "目标请求 ID")),
    responses(
        (status = 200, description = "同一 request_id 的审计事件链路", body = Vec<AuditLogItem>),
        (status = 400, description = "request_id 参数非法"),
        (status = 403, description = "缺少 audit:read 权限")
    )
)]
async fn trace_request_audit_logs(
    State(pool): State<&'static PgPool>,
    session: UserSession,
    Path(request_id): Path<String>,
) -> JsonResult<Vec<AuditLogItem>> {
    ensure_can_read_audit(&session)?;
    validate_request_id(&request_id)?;

    let rows = AuditLogModel::new()
        .list_by_request(pool, &request_id)
        .await?;
    Ok(Json(rows.into_iter().map(AuditLogItem::from).collect()))
}

/// 构建审计日志查询路由（挂载于 `/audit-logs`）。
pub fn new_audit_router(pool: &'static PgPool) -> Router {
    Router::new()
        .route("/", get(list_audit_logs))
        .route("/trace/{request_id}", get(trace_request_audit_logs))
        .with_state(pool)
}

#[derive(OpenApi)]
#[openapi(
    paths(list_audit_logs, trace_request_audit_logs),
    components(schemas(AuditLogItem, AuditLogListResp)),
    tags((name = "audit", description = "系统审计日志检索与链路追踪"))
)]
struct AuditApiDoc;

/// 返回审计日志路由的 OpenAPI 文档片段。
pub fn openapi() -> utoipa::openapi::OpenApi {
    AuditApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_read_audit_respects_admin_and_permissions() {
        assert!(can_read_audit(true, &[]));
        assert!(can_read_audit(false, &["audit:read".to_string()]));
        assert!(can_read_audit(false, &["audit:*".to_string()]));
        assert!(can_read_audit(false, &["*".to_string()]));
        assert!(!can_read_audit(false, &[]));
        assert!(!can_read_audit(false, &["rbac:read".to_string()]));
    }

    #[test]
    fn validate_request_id_enforces_bounds() {
        assert!(validate_request_id("req-123_abc").is_ok());
        let empty_err = validate_request_id("").expect_err("empty request_id must fail");
        assert_eq!(empty_err.status(), 400);
        assert_eq!(empty_err.sub_category(), Some("invalid_request_id"));

        let long = "x".repeat(MAX_REQUEST_ID_LEN + 1);
        let long_err = validate_request_id(&long).expect_err("oversized request_id must fail");
        assert_eq!(long_err.status(), 400);
        assert_eq!(long_err.sub_category(), Some("invalid_request_id"));
    }

    #[test]
    fn parse_optional_time_handles_empty_and_invalid_values() {
        assert_eq!(parse_optional_time(None).expect("none ok"), None);
        assert_eq!(
            parse_optional_time(Some("  ".to_string())).expect("blank ok"),
            None
        );
        assert!(parse_optional_time(Some("2026-06-05T12:34:56Z".to_string())).is_ok());
        let err = parse_optional_time(Some("not-a-date".to_string()))
            .expect_err("invalid datetime must fail");
        assert_eq!(err.status(), 400);
    }
}
