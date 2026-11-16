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

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;
use tibba_auth::{
    Role, Session, SessionParams, UserSession, permission_grants, revoke_role_sessions,
};
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_model::{
    AuditLogModel, AuditLogParams, Permission, PermissionModel, RolePermissionModel,
};
use tibba_util::{JsonParams, JsonResult};
use utoipa::{OpenApi, ToSchema};
use validator::Validate;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "rbac";
const MAX_PERMISSION_CODE_LEN: usize = 128;
const MAX_ROLE_NAME_LEN: usize = 64;
const MAX_DESCRIPTION_LEN: usize = 255;

/// 脚手架内置权限码字典（启动期幂等插入 `permissions` 表，不覆盖已有描述）。
pub const BUILTIN_PERMISSIONS: &[(&str, &str)] = &[
    ("model:user:read", "Read user records in model management"),
    ("model:user:write", "Create, update, or delete user records"),
    (
        "model:configuration:read",
        "Read dynamic configurations in model management",
    ),
    (
        "model:configuration:write",
        "Create, update, or delete dynamic configurations",
    ),
    ("rbac:read", "View permissions and role-permission mappings"),
    (
        "rbac:write",
        "Manage permissions and role-permission grants",
    ),
    ("audit:read", "Query system audit logs"),
];

fn rbac_error(sub_category: &str, message: impl Into<String>, status: u16) -> BaseError {
    BaseError::new(message.into())
        .with_category(ERROR_CATEGORY)
        .with_sub_category(sub_category)
        .with_status(status)
        .with_exception(false)
}

/// 启动期幂等补齐内置权限点（仅插入缺失条目，不覆盖管理员已自定义的描述）。
pub async fn ensure_builtin_permissions(pool: &PgPool) -> Result<()> {
    let model = PermissionModel::new();
    for &(code, desc) in BUILTIN_PERMISSIONS {
        model.insert_if_absent(pool, code, desc).await?;
    }
    Ok(())
}

/// 校验权限码格式（`1..=128` 位 ASCII 字母/数字及 `:` `_` `-` `*`），非法则返回 400。
pub fn validate_permission_code(code: &str) -> Result<()> {
    if code.is_empty()
        || code.len() > MAX_PERMISSION_CODE_LEN
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'-' | b'*'))
    {
        return Err(BaseError::new("invalid permission code")
            .with_category("rbac")
            .with_sub_category("invalid_permission_code")
            .with_status(400)
            .with_exception(false));
    }
    Ok(())
}

/// 校验角色名格式（`1..=64` 位 ASCII 字母/数字及 `_` `-`），非法则返回 400。
fn validate_role_name(role: &str) -> Result<()> {
    if role.is_empty()
        || role.len() > MAX_ROLE_NAME_LEN
        || !role
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(BaseError::new("invalid role name")
            .with_category("rbac")
            .with_sub_category("invalid_role_name")
            .with_status(400)
            .with_exception(false));
    }
    Ok(())
}

/// 角色权限管理：写库之后同步处理已签发凭证。
///
/// Session / JWT 在签发时缓存了权限并集。收回权限（撤销授予、删除权限点）后
/// 若不处理，持有该角色的用户在凭证过期前（Session 默认 7 天）仍保有被收回的
/// 权限。这里在收回成功后撤销受影响角色的凭证，相关用户需重新登录。
///
/// 授予权限不撤销凭证：不涉及安全收窄，新权限在下次登录时生效，避免一次授权
/// 就把整个角色的用户全部踢下线。
#[derive(Clone)]
pub struct RbacAdmin {
    pool: &'static PgPool,
    cache: &'static RedisCache,
    session_params: Arc<SessionParams>,
}

impl fmt::Debug for RbacAdmin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RbacAdmin").finish_non_exhaustive()
    }
}

impl RbacAdmin {
    pub fn new(
        pool: &'static PgPool,
        cache: &'static RedisCache,
        session_params: Arc<SessionParams>,
    ) -> Self {
        Self {
            pool,
            cache,
            session_params,
        }
    }

    /// 授予角色权限码（幂等）。已登录用户在重新登录后获得新权限。
    pub async fn grant(&self, role: &str, permission_code: &str) -> Result<()> {
        validate_role_name(role)?;
        validate_permission_code(permission_code)?;
        RolePermissionModel::new()
            .grant(self.pool, role, permission_code)
            .await?;
        Ok(())
    }

    /// 收回角色的权限码；确有收回时撤销该角色的已登录凭证。返回是否收回。
    pub async fn revoke(&self, role: &str, permission_code: &str) -> Result<bool> {
        validate_role_name(role)?;
        validate_permission_code(permission_code)?;
        let affected = RolePermissionModel::new()
            .revoke(self.pool, role, permission_code)
            .await?;
        if affected == 0 {
            return Ok(false);
        }
        revoke_role_sessions(self.cache, &self.session_params, role).await?;
        Ok(true)
    }

    /// 删除权限点并级联收回所有角色的授予，撤销这些角色的已登录凭证。
    /// 返回被收回该权限的角色列表。
    pub async fn delete_permission(&self, code: &str) -> Result<Vec<String>> {
        validate_permission_code(code)?;
        let roles = PermissionModel::new()
            .soft_delete_by_code(self.pool, code)
            .await?;
        for role in &roles {
            revoke_role_sessions(self.cache, &self.session_params, role).await?;
        }
        Ok(roles)
    }
}

fn has_super_admin_role(roles: &[String]) -> bool {
    roles
        .iter()
        .any(|r| Role::from(r.as_str()) == Role::SuperAdmin)
}

fn can_read_rbac_with(is_admin: bool, permissions: &[String]) -> bool {
    is_admin
        || permission_grants(permissions, "rbac:read")
        || permission_grants(permissions, "rbac:write")
}

fn can_write_rbac_with(roles: &[String], permissions: &[String]) -> bool {
    has_super_admin_role(roles) || permission_grants(permissions, "rbac:write")
}

fn ensure_can_read_rbac(session: &Session) -> Result<()> {
    if can_read_rbac_with(session.is_admin(), session.get_permissions()) {
        Ok(())
    } else {
        Err(rbac_error(
            "permission_denied",
            "rbac read permission required",
            403,
        ))
    }
}

fn ensure_can_write_rbac(session: &Session) -> Result<bool> {
    let is_su = has_super_admin_role(session.get_roles());
    if can_write_rbac_with(session.get_roles(), session.get_permissions()) {
        Ok(is_su)
    } else {
        Err(rbac_error(
            "permission_denied",
            "super admin or rbac:write permission required",
            403,
        ))
    }
}

/// 防止通过 RBAC 管理接口进行权限提升：
/// - `"*"` 仅供内置超管角色隐式持有，禁止建入库或授予其它角色（400）
/// - 非 `su` 持有者禁止操作任意通配符权限或 `rbac:write` 自身（403）
pub(crate) fn check_permission_write_guard(caller_is_super_admin: bool, code: &str) -> Result<()> {
    if code == "*" {
        return Err(rbac_error(
            "wildcard_reserved",
            "wildcard permission '*' is reserved for super admin",
            400,
        ));
    }
    if !caller_is_super_admin && (code.contains('*') || code == "rbac:write") {
        return Err(rbac_error(
            "wildcard_forbidden",
            "only super admin can manage wildcard or rbac:write permissions",
            403,
        ));
    }
    Ok(())
}

/// 禁止修改内置 `su` 角色的权限授予（`su` 固定隐式持有 `*`）。
fn check_role_write_guard(role: &str) -> Result<()> {
    validate_role_name(role)?;
    if Role::from(role) == Role::SuperAdmin {
        return Err(rbac_error(
            "super_admin_immutable",
            "super admin role permissions cannot be modified",
            400,
        ));
    }
    Ok(())
}

#[derive(Clone)]
struct RbacRouterState {
    admin: RbacAdmin,
    pool: &'static PgPool,
}

#[derive(Debug, Serialize, ToSchema)]
struct PermissionItem {
    id: i64,
    code: String,
    description: String,
    created: String,
    modified: String,
}

impl From<Permission> for PermissionItem {
    fn from(p: Permission) -> Self {
        Self {
            id: p.id,
            code: p.code,
            description: p.description,
            created: p.created,
            modified: p.modified,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
struct RoleSummary {
    role: String,
    immutable: bool,
    permissions: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
struct RbacOverviewResp {
    permissions: Vec<PermissionItem>,
    roles: Vec<RoleSummary>,
    can_write: bool,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
struct UpsertPermissionBody {
    /// 权限码（1..=128 ASCII 字母数字及 `: _ - *`）
    #[validate(length(min = 1, max = 128))]
    code: String,
    /// 权限说明
    #[validate(length(max = 255))]
    #[serde(default)]
    description: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct UpsertPermissionResp {
    id: i64,
    code: String,
    description: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
struct GrantRolePermissionBody {
    /// 待授予的权限码
    #[validate(length(min = 1, max = 128))]
    permission_code: String,
}

#[allow(clippy::too_many_arguments)]
async fn log_rbac_audit(
    pool: &PgPool,
    session: &Session,
    action: &str,
    target_type: &str,
    target_id: &str,
    request_id: &RequestId,
    ip: std::net::IpAddr,
    headers: &HeaderMap,
    detail: serde_json::Value,
) {
    let _ = AuditLogModel::new()
        .log(
            pool,
            AuditLogParams::new(action)
                .with_user(session.get_user_id())
                .with_target(target_type, target_id)
                .with_request(
                    request_id.as_str(),
                    ip.to_string(),
                    crate::user_agent_of(headers),
                )
                .with_detail(detail),
        )
        .await;
}

/// 获取权限字典、已知角色及角色授权矩阵。
#[utoipa::path(
    get,
    path = "/rbac/overview",
    tag = "rbac",
    responses(
        (status = 200, description = "RBAC 权限字典与角色授权概览", body = RbacOverviewResp),
        (status = 403, description = "缺少 rbac:read 权限")
    )
)]
async fn get_overview(
    State(state): State<RbacRouterState>,
    session: UserSession,
) -> JsonResult<RbacOverviewResp> {
    ensure_can_read_rbac(&session)?;
    let can_write = can_write_rbac_with(session.get_roles(), session.get_permissions());

    let perms = PermissionModel::new().list_all(state.pool).await?;
    let rp_model = RolePermissionModel::new();
    let mappings = rp_model.list_all(state.pool).await?;
    let known_roles = rp_model.list_known_roles(state.pool).await?;

    let mut role_perms_map: HashMap<String, Vec<String>> = HashMap::new();
    for item in mappings {
        role_perms_map
            .entry(item.role)
            .or_default()
            .push(item.permission_code);
    }

    let mut extra_roles = BTreeSet::new();
    for r in known_roles {
        if r != "su" && r != "admin" {
            extra_roles.insert(r);
        }
    }

    let mut roles = Vec::with_capacity(extra_roles.len() + 2);
    roles.push(RoleSummary {
        role: "su".to_string(),
        immutable: true,
        permissions: vec!["*".to_string()],
    });
    roles.push(RoleSummary {
        role: "admin".to_string(),
        immutable: false,
        permissions: role_perms_map.remove("admin").unwrap_or_default(),
    });
    for role in extra_roles {
        let permissions = role_perms_map.remove(&role).unwrap_or_default();
        roles.push(RoleSummary {
            role,
            immutable: false,
            permissions,
        });
    }

    Ok(Json(RbacOverviewResp {
        permissions: perms.into_iter().map(PermissionItem::from).collect(),
        roles,
        can_write,
    }))
}

/// 新增或更新权限点（幂等）。
#[utoipa::path(
    post,
    path = "/rbac/permissions",
    tag = "rbac",
    request_body = UpsertPermissionBody,
    responses(
        (status = 200, description = "权限点已保存", body = UpsertPermissionResp),
        (status = 400, description = "权限码非法或包含保留字符"),
        (status = 403, description = "无写权限或越权操作通配权限")
    )
)]
async fn upsert_permission(
    State(state): State<RbacRouterState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(params): JsonParams<UpsertPermissionBody>,
) -> JsonResult<UpsertPermissionResp> {
    let is_su = ensure_can_write_rbac(&session)?;
    validate_permission_code(&params.code)?;
    check_permission_write_guard(is_su, &params.code)?;

    let description = params.description.trim().to_string();
    if description.chars().count() > MAX_DESCRIPTION_LEN {
        return Err(rbac_error(
            "invalid_description",
            "permission description too long",
            400,
        ));
    }

    let id = PermissionModel::new()
        .upsert(state.pool, &params.code, &description)
        .await?;

    log_rbac_audit(
        state.pool,
        &session,
        "rbac.permission_upsert",
        "permission",
        &params.code,
        &request_id,
        ip,
        &headers,
        json!({ "description": description }),
    )
    .await;

    Ok(Json(UpsertPermissionResp {
        id,
        code: params.code,
        description,
    }))
}

/// 软删除权限点并级联撤销所有角色授予，同时让受影响角色的在线会话失效。
#[utoipa::path(
    delete,
    path = "/rbac/permissions/{code}",
    tag = "rbac",
    params(("code" = String, Path, description = "权限码")),
    responses(
        (status = 204, description = "权限点已删除并完成级联撤权"),
        (status = 400, description = "权限码非法"),
        (status = 403, description = "无写权限或越权删除关键权限")
    )
)]
async fn delete_permission_handler(
    State(state): State<RbacRouterState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<StatusCode> {
    let is_su = ensure_can_write_rbac(&session)?;
    validate_permission_code(&code)?;
    check_permission_write_guard(is_su, &code)?;

    let affected_roles = state.admin.delete_permission(&code).await?;

    log_rbac_audit(
        state.pool,
        &session,
        "rbac.permission_delete",
        "permission",
        &code,
        &request_id,
        ip,
        &headers,
        json!({ "affected_roles": affected_roles }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 为指定角色授予权限码（幂等）。
#[utoipa::path(
    post,
    path = "/rbac/roles/{role}/permissions",
    tag = "rbac",
    params(("role" = String, Path, description = "目标角色名（不可为 su）")),
    request_body = GrantRolePermissionBody,
    responses(
        (status = 204, description = "授权成功"),
        (status = 400, description = "角色名/权限码非法或尝试修改 su"),
        (status = 403, description = "无写权限或越权授予通配权限"),
        (status = 404, description = "权限码尚未注册")
    )
)]
async fn grant_role_permission(
    State(state): State<RbacRouterState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(role): Path<String>,
    JsonParams(params): JsonParams<GrantRolePermissionBody>,
) -> Result<StatusCode> {
    let is_su = ensure_can_write_rbac(&session)?;
    check_role_write_guard(&role)?;
    validate_permission_code(&params.permission_code)?;
    check_permission_write_guard(is_su, &params.permission_code)?;

    if PermissionModel::new()
        .get_by_code(state.pool, &params.permission_code)
        .await?
        .is_none()
    {
        return Err(rbac_error(
            "permission_not_found",
            format!("permission not found: {}", params.permission_code),
            404,
        ));
    }

    state.admin.grant(&role, &params.permission_code).await?;

    log_rbac_audit(
        state.pool,
        &session,
        "rbac.role_grant",
        "role",
        &role,
        &request_id,
        ip,
        &headers,
        json!({ "permission_code": params.permission_code }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 收回指定角色的权限码，并在确有变更时使该角色已登录会话失效。
#[utoipa::path(
    delete,
    path = "/rbac/roles/{role}/permissions/{code}",
    tag = "rbac",
    params(
        ("role" = String, Path, description = "目标角色名（不可为 su）"),
        ("code" = String, Path, description = "待收回的权限码")
    ),
    responses(
        (status = 204, description = "撤权完成"),
        (status = 400, description = "角色名/权限码非法或尝试修改 su"),
        (status = 403, description = "无写权限或越权操作通配权限")
    )
)]
async fn revoke_role_permission(
    State(state): State<RbacRouterState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path((role, code)): Path<(String, String)>,
) -> Result<StatusCode> {
    let is_su = ensure_can_write_rbac(&session)?;
    check_role_write_guard(&role)?;
    validate_permission_code(&code)?;
    check_permission_write_guard(is_su, &code)?;

    let revoked = state.admin.revoke(&role, &code).await?;

    log_rbac_audit(
        state.pool,
        &session,
        "rbac.role_revoke",
        "role",
        &role,
        &request_id,
        ip,
        &headers,
        json!({ "permission_code": code, "revoked": revoked }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 构建 RBAC 管理路由（挂载于 `/rbac`）。
pub fn new_rbac_router(admin: RbacAdmin, pool: &'static PgPool) -> Router {
    let state = RbacRouterState { admin, pool };
    Router::new()
        .route("/overview", get(get_overview))
        .route("/permissions", post(upsert_permission))
        .route("/permissions/{code}", delete(delete_permission_handler))
        .route("/roles/{role}/permissions", post(grant_role_permission))
        .route(
            "/roles/{role}/permissions/{code}",
            delete(revoke_role_permission),
        )
        .with_state(state)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_overview,
        upsert_permission,
        delete_permission_handler,
        grant_role_permission,
        revoke_role_permission,
    ),
    components(schemas(
        PermissionItem,
        RoleSummary,
        RbacOverviewResp,
        UpsertPermissionBody,
        UpsertPermissionResp,
        GrantRolePermissionBody,
    )),
    tags((name = "rbac", description = "RBAC 权限字典与角色授权管理"))
)]
struct RbacApiDoc;

/// 返回 RBAC 路由的 OpenAPI 文档片段。
pub fn openapi() -> utoipa::openapi::OpenApi {
    RbacApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_code_validation_rejects_bad_inputs() {
        for ok in ["model:read", "file:upload-1", "user_admin:write", "*"] {
            assert!(validate_permission_code(ok).is_ok(), "{ok} should pass");
        }
        for bad in ["", "bad space", "bad/slash", &"a".repeat(129)] {
            let err = validate_permission_code(bad).expect_err("bad code must fail");
            assert_eq!(err.status(), 400);
            assert!(!err.is_exception());
            assert_eq!(err.sub_category(), Some("invalid_permission_code"));
        }
    }

    #[test]
    fn role_name_validation_rejects_bad_inputs() {
        assert!(validate_role_name("admin").is_ok());
        assert!(validate_role_name("read_only-1").is_ok());
        let err = validate_role_name("bad:role").expect_err("colon invalid in role");
        assert_eq!(err.status(), 400);
        assert_eq!(err.sub_category(), Some("invalid_role_name"));
    }

    #[test]
    fn rbac_read_write_guards_enforce_least_privilege() {
        let empty: Vec<String> = vec![];
        let su_roles = vec!["su".to_string()];
        let admin_roles = vec!["admin".to_string()];
        let editor_roles = vec!["editor".to_string()];

        // 普通 admin 可读不可写（除非显式授予 rbac:write）
        assert!(can_read_rbac_with(true, &empty));
        assert!(!can_write_rbac_with(&admin_roles, &empty));
        assert!(can_write_rbac_with(
            &admin_roles,
            &["rbac:write".to_string()]
        ));

        // su 既可读也可写
        assert!(can_write_rbac_with(&su_roles, &empty));

        // 普通角色持有 rbac:read 仅可读；无权限均拒绝
        assert!(can_read_rbac_with(false, &["rbac:read".to_string()]));
        assert!(!can_read_rbac_with(false, &["audit:read".to_string()]));
        assert!(!can_write_rbac_with(
            &editor_roles,
            &["rbac:read".to_string()]
        ));
    }

    #[test]
    fn rbac_anti_escalation_blocks_wildcards_and_su_mutation() {
        // 任何人都不允许将 "*" 写入权限表或授予角色
        for is_su in [true, false] {
            let err = check_permission_write_guard(is_su, "*").expect_err("* must be reserved");
            assert_eq!(err.status(), 400);
            assert_eq!(err.sub_category(), Some("wildcard_reserved"));
        }

        // 非 su 禁止操作含 * 通配符及 rbac:write
        for forbidden in ["rbac:*", "model:*", "rbac:write"] {
            let err = check_permission_write_guard(false, forbidden)
                .expect_err("non-su must not manage wildcards or rbac:write");
            assert_eq!(err.status(), 403);
            assert_eq!(err.sub_category(), Some("wildcard_forbidden"));
        }

        // su 可以管理细粒度与前缀通配权限，非 su 可以管理普通权限
        assert!(check_permission_write_guard(true, "rbac:write").is_ok());
        assert!(check_permission_write_guard(true, "model:*").is_ok());
        assert!(check_permission_write_guard(false, "model:user:read").is_ok());

        // su 角色禁止修改
        let err = check_role_write_guard("su").expect_err("su role immutable");
        assert_eq!(err.status(), 400);
        assert_eq!(err.sub_category(), Some("super_admin_immutable"));
        assert!(check_role_write_guard("admin").is_ok());
        assert!(check_role_write_guard("auditor").is_ok());
    }
}
