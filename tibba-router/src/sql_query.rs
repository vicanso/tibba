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

//! 运行期配置化只读 SQL 查询接口（feature = `"sql-query"`）：
//!
//! - 配置存储：复用 `configurations` 表（固定 `category = "sql_query"`）+ `ConfigStore` 双层缓存与跨节点失效广播；
//! - 管理端点（`/sql-queries`，仅 `su` 超级管理员）：列表、创建、更新、软删除与在线带参试运行；
//! - 执行端点（`/queries/{group}/{name}`）：按配置声明的访问级别（`admin` / `permission` / `login` / `public`）鉴权，
//!   强制预编译参数绑定 + 会话上下文注入（`current_user_id` / `current_account`），
//!   在 `SET LOCAL transaction_read_only = on` 与 `SET LOCAL statement_timeout` 事务内以 `to_jsonb` 执行只读查询。

use crate::configuration::{ConfigStore, SQL_QUERY_CATEGORY, SQL_QUERY_GROUP_CATEGORY};
use crate::rbac::validate_permission_code;
use crate::user_agent_of;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, types::Json as SqlxJson};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tibba_auth::{Role, Session, UserSession};
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;
use tibba_middleware::{ClientIp, RequestId};
use tibba_model::{
    AuditLogModel, AuditLogParams, Configuration, ConfigurationModel, Model, Status,
    format_datetime, now_primitive_utc, parse_primitive_datetime,
};
use tibba_util::{JsonParams, JsonResult, sha256};
use time::PrimitiveDateTime;
use tracing::warn;
use utoipa::{OpenApi, ToSchema};
use validator::Validate;

type Result<T, E = BaseError> = std::result::Result<T, E>;

const LOG_TARGET: &str = "tibba:sql_query";
const ERROR_CATEGORY: &str = "sql_query";

const MAX_QUERY_GROUP_LEN: usize = 32;
const MAX_QUERY_NAME_LEN: usize = 64;
const MAX_PARAM_NAME_LEN: usize = 64;
const MAX_PARAMS_COUNT: usize = 32;
const MAX_SQL_LEN: usize = 16 * 1024;
const MAX_STRING_ARG_LEN: usize = 1024;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_TIMEOUT_MS: u64 = 3_000;
const MIN_MAX_ROWS: i64 = 1;
const MAX_MAX_ROWS: i64 = 1_000;
const DEFAULT_MAX_ROWS: i64 = 200;
const MAX_CACHE_TTL_SECS: u64 = 86_400;

const DEFAULT_END_TIME: &str = "2099-12-31T23:59:59Z";

const FORBIDDEN_SQL_KEYWORDS: &[&str] = &[
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "DROP",
    "ALTER",
    "CREATE",
    "TRUNCATE",
    "GRANT",
    "REVOKE",
    "CALL",
    "DO",
    "LOCK",
    "VACUUM",
    "ANALYZE",
    "CLUSTER",
    "REINDEX",
    "COPY",
    "SET",
    "RESET",
    "EXECUTE",
    "PREPARE",
    "DEALLOCATE",
    "LISTEN",
    "NOTIFY",
];

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn default_max_rows() -> i64 {
    DEFAULT_MAX_ROWS
}

fn default_start_time() -> String {
    format_datetime(now_primitive_utc())
}

fn query_error(
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

fn is_valid_identifier(value: &str, max_len: usize, allow_dash: bool) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || (allow_dash && b == b'-'))
}

fn validate_query_group(group: &str) -> Result<()> {
    if !is_valid_identifier(group, MAX_QUERY_GROUP_LEN, true) {
        return Err(query_error(
            "invalid_param",
            "invalid query group (1..=32 ASCII letters, digits, '_' or '-')",
            400,
            false,
        ));
    }
    Ok(())
}

fn validate_query_name(name: &str) -> Result<()> {
    if !is_valid_identifier(name, MAX_QUERY_NAME_LEN, true) {
        return Err(query_error(
            "invalid_param",
            "invalid query name (1..=64 ASCII letters, digits, '_' or '-')",
            400,
            false,
        ));
    }
    Ok(())
}

fn format_query_key(group: &str, name: &str) -> String {
    format!("{group}/{name}")
}

fn split_query_key(full_name: &str) -> (String, String) {
    match full_name.split_once('/') {
        Some((group, name)) if !group.is_empty() && !name.is_empty() => {
            (group.to_string(), name.to_string())
        }
        _ => ("default".to_string(), full_name.to_string()),
    }
}

fn check_role_allowed(is_login: bool, roles: &[String], require_su: bool) -> Result<()> {
    if !is_login {
        return Err(query_error("not_logged_in", "login required", 401, false));
    }
    let allowed = roles.iter().any(|r| {
        let role = Role::from(r.as_str());
        if require_su {
            role == Role::SuperAdmin
        } else {
            matches!(role, Role::SuperAdmin | Role::Admin)
        }
    });
    if !allowed {
        let (sub_category, message) = if require_su {
            (
                "privilege_escalation",
                "only super admin can manage sql queries",
            )
        } else {
            (
                "not_admin",
                "only admin or super admin can manage sql query groups",
            )
        };
        return Err(query_error(sub_category, message, 403, false));
    }
    Ok(())
}

fn ensure_super_admin(session: &Session) -> Result<()> {
    check_role_allowed(session.is_login(), session.get_roles(), true)
}

fn ensure_admin(session: &Session) -> Result<()> {
    check_role_allowed(session.is_login(), session.get_roles(), false)
}

/// 生成接口的访问鉴权级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SqlAccessLevel {
    #[default]
    Admin,
    Permission,
    Login,
    Public,
}

/// 查询结果返回形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SqlResultMode {
    #[default]
    List,
    Single,
}

/// 参数类型定义（URL 入参或服务端强注入的当前会话上下文）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SqlParamType {
    Int,
    Float,
    Bool,
    String,
    Datetime,
    CurrentUserId,
    CurrentAccount,
}

impl SqlParamType {
    #[must_use]
    pub fn is_session_bound(self) -> bool {
        matches!(self, Self::CurrentUserId | Self::CurrentAccount)
    }
}

/// 单个位置绑定参数（按顺序对应 PostgreSQL `$1, $2, ...`）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SqlParamDef {
    /// URL query 参数名（1..=64 位 ASCII 字母数字与 `_`）；会话注入类型仅作展示标签
    pub name: String,
    /// 参数类型
    pub param_type: SqlParamType,
    /// 是否必填（对会话类型无意义，会话类型始终要求已登录）
    #[serde(default)]
    pub required: bool,
    /// 可选默认值字符串（URL 未传或为空时回退）
    #[serde(default)]
    pub default_value: Option<String>,
    /// 数值型参数（`int` / `float`）可选下界
    #[serde(default)]
    pub min: Option<f64>,
    /// 数值型参数（`int` / `float`）可选上界
    #[serde(default)]
    pub max: Option<f64>,
}

/// `configurations.data`（`category = "sql_query"`）的 JSON 结构。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SqlQuerySpec {
    /// 只读 SQL 文本（仅允许单条 `SELECT` / `WITH ... SELECT`）
    pub sql: String,
    /// 按 `$1, $2, ...` 顺序定义的参数列表
    #[serde(default)]
    pub params: Vec<SqlParamDef>,
    /// 接口访问鉴权级别（默认 `admin`）
    #[serde(default)]
    pub access: SqlAccessLevel,
    /// 当 `access == "permission"` 时必填的 RBAC 权限码
    #[serde(default)]
    pub permission_code: String,
    /// 结果形态：`list` 返回数组，`single` 返回首行对象或 `null`
    #[serde(default)]
    pub result_mode: SqlResultMode,
    /// SQL 执行超时（毫秒，100..=15000，默认 3000）
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// 列表模式返回行数上限（1..=1000，默认 200）
    #[serde(default = "default_max_rows")]
    pub max_rows: i64,
    /// Redis 结果缓存 TTL（秒，0 表示不缓存，最大 86400）
    #[serde(default)]
    pub cache_ttl_secs: u64,
}

/// 校验并规范化只读 SQL：
/// - 剔除首尾空白与至多一个末尾 `;`
/// - 词法扫描注释（`--`、`/* ... */`）与引号（`'...'`、`E'...'`、`"..."`）
/// - 拒绝多语句分号、未闭合注释/引号、dollar-quote、首关键字非 `SELECT`/`WITH` 及显式写操作/DDL 关键字
/// - 提取所有 `$N` 占位符索引并校验是否在 `1..=param_count` 范围内
pub(crate) fn validate_readonly_sql(raw: &str, param_count: usize) -> Result<String> {
    let trimmed = raw.trim();
    let without_trailing_semi = trimmed
        .strip_suffix(';')
        .map(str::trim_end)
        .unwrap_or(trimmed)
        .trim();

    if without_trailing_semi.is_empty() || without_trailing_semi.len() > MAX_SQL_LEN {
        return Err(query_error(
            "invalid_sql",
            "sql must be non-empty and at most 16KB",
            400,
            false,
        ));
    }

    let bytes = without_trailing_semi.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut first_keyword: Option<String> = None;

    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // 单行注释 --
        if b == b'-' && i + 1 < len && bytes[i + 1] == b'-' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // 块注释 /* ... */（支持嵌套）
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            let mut depth = 1usize;
            while i < len && depth > 0 {
                if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth > 0 {
                return Err(query_error(
                    "invalid_sql",
                    "unclosed block comment in sql",
                    400,
                    false,
                ));
            }
            continue;
        }
        // PostgreSQL C 风格转义字符串 E'...' / e'...'
        if (b == b'e' || b == b'E') && i + 1 < len && bytes[i + 1] == b'\'' {
            if first_keyword.is_none() {
                return Err(query_error(
                    "invalid_sql",
                    "sql must start with SELECT or WITH",
                    400,
                    false,
                ));
            }
            i += 2;
            let mut closed = false;
            while i < len {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(len);
                    continue;
                }
                if bytes[i] == b'\'' {
                    if i + 1 < len && bytes[i + 1] == b'\'' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err(query_error(
                    "invalid_sql",
                    "unclosed string literal in sql",
                    400,
                    false,
                ));
            }
            continue;
        }
        // 单引号字符串字面量 '...'
        if b == b'\'' {
            if first_keyword.is_none() {
                return Err(query_error(
                    "invalid_sql",
                    "sql must start with SELECT or WITH",
                    400,
                    false,
                ));
            }
            i += 1;
            let mut closed = false;
            while i < len {
                if bytes[i] == b'\'' {
                    if i + 1 < len && bytes[i + 1] == b'\'' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err(query_error(
                    "invalid_sql",
                    "unclosed string literal in sql",
                    400,
                    false,
                ));
            }
            continue;
        }
        // 双引号标识符 "..."
        if b == b'"' {
            if first_keyword.is_none() {
                return Err(query_error(
                    "invalid_sql",
                    "sql must start with SELECT or WITH",
                    400,
                    false,
                ));
            }
            i += 1;
            let mut closed = false;
            while i < len {
                if bytes[i] == b'"' {
                    if i + 1 < len && bytes[i + 1] == b'"' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err(query_error(
                    "invalid_sql",
                    "unclosed quoted identifier in sql",
                    400,
                    false,
                ));
            }
            continue;
        }
        // 禁止任意语句内/语句间未引号分号
        if b == b';' {
            return Err(query_error(
                "invalid_sql",
                "multiple sql statements or extra semicolons are not allowed",
                400,
                false,
            ));
        }
        // $ 仅允许 $1..$N 参数占位符，拒绝 $$ / $tag$ dollar-quoting
        if b == b'$' {
            if first_keyword.is_none() {
                return Err(query_error(
                    "invalid_sql",
                    "sql must start with SELECT or WITH",
                    400,
                    false,
                ));
            }
            let start = i + 1;
            let mut end = start;
            while end < len && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end == start
                || (end < len
                    && (bytes[end].is_ascii_alphanumeric()
                        || bytes[end] == b'_'
                        || bytes[end] == b'$'))
            {
                return Err(query_error(
                    "invalid_sql",
                    "dollar-quoted strings or malformed placeholders are not allowed",
                    400,
                    false,
                ));
            }
            let index: usize = without_trailing_semi[start..end].parse().map_err(|_| {
                query_error("invalid_sql", "invalid parameter placeholder", 400, false)
            })?;
            if index == 0 || index > param_count {
                return Err(query_error(
                    "invalid_sql",
                    format!(
                        "placeholder ${index} references out-of-range parameter (defined: {param_count})"
                    ),
                    400,
                    false,
                ));
            }
            i = end;
            continue;
        }
        // 关键字或普通标识符
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &without_trailing_semi[start..i];
            let upper = word.to_ascii_uppercase();
            if first_keyword.is_none() {
                if upper != "SELECT" && upper != "WITH" {
                    return Err(query_error(
                        "invalid_sql",
                        "sql must start with SELECT or WITH",
                        400,
                        false,
                    ));
                }
                first_keyword = Some(upper);
            } else if FORBIDDEN_SQL_KEYWORDS.contains(&upper.as_str()) {
                return Err(query_error(
                    "invalid_sql",
                    format!("keyword '{upper}' is not allowed in read-only sql"),
                    400,
                    false,
                ));
            }
            continue;
        }
        if first_keyword.is_none() {
            return Err(query_error(
                "invalid_sql",
                "sql must start with SELECT or WITH",
                400,
                false,
            ));
        }
        i += 1;
    }

    if first_keyword.is_none() {
        return Err(query_error(
            "invalid_sql",
            "sql must start with SELECT or WITH",
            400,
            false,
        ));
    }

    Ok(without_trailing_semi.to_string())
}

/// 解析并校验单个参数值（供 runtime 执行与保存 spec 时默认值自检共用）。
fn parse_typed_arg(def: &SqlParamDef, raw: &str) -> Result<(BoundValue, Value)> {
    match def.param_type {
        SqlParamType::Int => {
            let val: i64 = raw.trim().parse().map_err(|_| {
                query_error(
                    "validation",
                    format!("parameter '{}' must be an integer", def.name),
                    400,
                    false,
                )
            })?;
            let as_f64 = val as f64;
            if let Some(min) = def.min
                && as_f64 < min
            {
                return Err(query_error(
                    "validation",
                    format!("parameter '{}' must be >= {min}", def.name),
                    400,
                    false,
                ));
            }
            if let Some(max) = def.max
                && as_f64 > max
            {
                return Err(query_error(
                    "validation",
                    format!("parameter '{}' must be <= {max}", def.name),
                    400,
                    false,
                ));
            }
            Ok((BoundValue::Int(Some(val)), json!(val)))
        }
        SqlParamType::Float => {
            let val: f64 = raw.trim().parse().map_err(|_| {
                query_error(
                    "validation",
                    format!("parameter '{}' must be a number", def.name),
                    400,
                    false,
                )
            })?;
            if !val.is_finite() {
                return Err(query_error(
                    "validation",
                    format!("parameter '{}' must be finite", def.name),
                    400,
                    false,
                ));
            }
            if let Some(min) = def.min
                && val < min
            {
                return Err(query_error(
                    "validation",
                    format!("parameter '{}' must be >= {min}", def.name),
                    400,
                    false,
                ));
            }
            if let Some(max) = def.max
                && val > max
            {
                return Err(query_error(
                    "validation",
                    format!("parameter '{}' must be <= {max}", def.name),
                    400,
                    false,
                ));
            }
            Ok((BoundValue::Float(Some(val)), json!(val)))
        }
        SqlParamType::Bool => {
            let lower = raw.trim().to_ascii_lowercase();
            let val = match lower.as_str() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => {
                    return Err(query_error(
                        "validation",
                        format!("parameter '{}' must be true/false/1/0", def.name),
                        400,
                        false,
                    ));
                }
            };
            Ok((BoundValue::Bool(Some(val)), json!(val)))
        }
        SqlParamType::String => {
            if raw.len() > MAX_STRING_ARG_LEN {
                return Err(query_error(
                    "validation",
                    format!(
                        "parameter '{}' exceeds max length {MAX_STRING_ARG_LEN}",
                        def.name
                    ),
                    400,
                    false,
                ));
            }
            Ok((BoundValue::Text(Some(raw.to_string())), json!(raw)))
        }
        SqlParamType::Datetime => {
            let dt = parse_primitive_datetime(raw.trim()).map_err(BaseError::from)?;
            Ok((BoundValue::Datetime(Some(dt)), json!(dt.to_string())))
        }
        SqlParamType::CurrentUserId | SqlParamType::CurrentAccount => Err(query_error(
            "invalid_spec",
            "session-bound parameter cannot parse external value",
            400,
            false,
        )),
    }
}

/// 校验完整 SQL 查询配置并返回规范化后的 SQL 字符串。
pub(crate) fn validate_sql_query_spec(spec: &SqlQuerySpec) -> Result<String> {
    if spec.params.len() > MAX_PARAMS_COUNT {
        return Err(query_error(
            "invalid_spec",
            format!("at most {MAX_PARAMS_COUNT} parameters are allowed"),
            400,
            false,
        ));
    }
    if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&spec.timeout_ms) {
        return Err(query_error(
            "invalid_spec",
            format!("timeout_ms must be between {MIN_TIMEOUT_MS} and {MAX_TIMEOUT_MS}"),
            400,
            false,
        ));
    }
    if !(MIN_MAX_ROWS..=MAX_MAX_ROWS).contains(&spec.max_rows) {
        return Err(query_error(
            "invalid_spec",
            format!("max_rows must be between {MIN_MAX_ROWS} and {MAX_MAX_ROWS}"),
            400,
            false,
        ));
    }
    if spec.cache_ttl_secs > MAX_CACHE_TTL_SECS {
        return Err(query_error(
            "invalid_spec",
            format!("cache_ttl_secs must be <= {MAX_CACHE_TTL_SECS}"),
            400,
            false,
        ));
    }

    let mut seen_url_names = HashSet::new();
    for param in &spec.params {
        if !is_valid_identifier(&param.name, MAX_PARAM_NAME_LEN, false) {
            return Err(query_error(
                "invalid_spec",
                format!(
                    "invalid parameter name '{}' (1..=64 ASCII letters, digits or '_')",
                    param.name
                ),
                400,
                false,
            ));
        }
        if param.param_type.is_session_bound() {
            if spec.access == SqlAccessLevel::Public {
                return Err(query_error(
                    "invalid_spec",
                    "public query cannot use current_user_id or current_account",
                    400,
                    false,
                ));
            }
            continue;
        }
        if !seen_url_names.insert(param.name.clone()) {
            return Err(query_error(
                "invalid_spec",
                format!("duplicate parameter name '{}'", param.name),
                400,
                false,
            ));
        }
        if let Some(min) = param.min
            && !min.is_finite()
        {
            return Err(query_error(
                "invalid_spec",
                "param min must be finite",
                400,
                false,
            ));
        }
        if let Some(max) = param.max
            && !max.is_finite()
        {
            return Err(query_error(
                "invalid_spec",
                "param max must be finite",
                400,
                false,
            ));
        }
        if let (Some(min), Some(max)) = (param.min, param.max)
            && min > max
        {
            return Err(query_error(
                "invalid_spec",
                format!("parameter '{}' min must be <= max", param.name),
                400,
                false,
            ));
        }
        if let Some(default_val) = param.default_value.as_deref().filter(|s| !s.is_empty()) {
            parse_typed_arg(param, default_val)?;
        }
    }

    if spec.access == SqlAccessLevel::Permission {
        validate_permission_code(&spec.permission_code)?;
        if spec.permission_code == "*" {
            return Err(query_error(
                "wildcard_reserved",
                "wildcard permission '*' cannot be used here",
                400,
                false,
            ));
        }
    }

    validate_readonly_sql(&spec.sql, spec.params.len())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BoundValue {
    Int(Option<i64>),
    Float(Option<f64>),
    Bool(Option<bool>),
    Text(Option<String>),
    Datetime(Option<PrimitiveDateTime>),
}

/// 基于显式身份字段校验访问权限（便于无 Redis/Cookie 环境单元测试）。
pub(crate) fn check_query_access_parts(
    spec: &SqlQuerySpec,
    is_login: bool,
    is_admin: bool,
    has_permission: bool,
) -> Result<()> {
    match spec.access {
        SqlAccessLevel::Public => Ok(()),
        SqlAccessLevel::Login => {
            if is_login {
                Ok(())
            } else {
                Err(query_error("not_logged_in", "login required", 401, false))
            }
        }
        SqlAccessLevel::Admin => {
            if !is_login {
                Err(query_error("not_logged_in", "login required", 401, false))
            } else if is_admin {
                Ok(())
            } else {
                Err(query_error("not_admin", "admin required", 403, false))
            }
        }
        SqlAccessLevel::Permission => {
            if !is_login {
                Err(query_error("not_logged_in", "login required", 401, false))
            } else if is_admin || has_permission {
                Ok(())
            } else {
                Err(query_error(
                    "permission_denied",
                    format!("permission '{}' required", spec.permission_code),
                    403,
                    false,
                ))
            }
        }
    }
}

/// 校验执行权限（默认 `admin`，亦支持 `permission` / `login` / `public`）。
pub(crate) fn check_query_access(spec: &SqlQuerySpec, session: &Session) -> Result<()> {
    let has_perm =
        spec.access == SqlAccessLevel::Permission && session.has_permission(&spec.permission_code);
    check_query_access_parts(spec, session.is_login(), session.is_admin(), has_perm)
}

struct OptionalAppClient(Option<crate::AppClientIdentity>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for OptionalAppClient {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        Ok(Self(
            parts.extensions.get::<crate::AppClientIdentity>().cloned(),
        ))
    }
}

/// 校验已通过应用签名鉴权的调用方是否有权执行 `{group}/{name}` 查询接口。
pub(crate) fn check_app_query_access(
    spec: &SqlQuerySpec,
    app: &crate::AppClientIdentity,
    group: &str,
    name: &str,
) -> Result<()> {
    if spec.access == SqlAccessLevel::Public || app.allows_query(group, name) {
        return Ok(());
    }
    Err(query_error(
        "permission_denied",
        format!(
            "app '{}' is not authorized for query '{group}/{name}'",
            app.app_id
        ),
        403,
        false,
    ))
}

/// 根据调用方身份（应用签名凭据优先于人类用户会话）校验 SQL 查询访问权限。
pub(crate) fn check_query_caller_access(
    spec: &SqlQuerySpec,
    app_client: Option<&crate::AppClientIdentity>,
    group: &str,
    name: &str,
    session: &Session,
) -> Result<()> {
    if let Some(app) = app_client {
        check_app_query_access(spec, app, group, name)
    } else {
        check_query_access(spec, session)
    }
}

/// 按定义顺序解析并绑定 URL 查询参数及服务端强注入的会话身份参数，同时生成确定性缓存摘要输入。
pub(crate) fn resolve_bound_params_with(
    defs: &[SqlParamDef],
    raw_query: &HashMap<String, String>,
    session_identity: Option<(i64, &str)>,
) -> Result<(Vec<BoundValue>, Value)> {
    let mut bound = Vec::with_capacity(defs.len());
    let mut canonical = Vec::with_capacity(defs.len());

    for def in defs {
        match def.param_type {
            SqlParamType::CurrentUserId => {
                let Some((uid, _)) = session_identity else {
                    return Err(query_error("not_logged_in", "login required", 401, false));
                };
                if uid <= 0 {
                    return Err(query_error(
                        "app_session_forbidden",
                        "current_user_id parameter requires a human user session",
                        403,
                        false,
                    ));
                }
                bound.push(BoundValue::Int(Some(uid)));
                canonical.push(json!(uid));
            }
            SqlParamType::CurrentAccount => {
                let Some((_, account)) = session_identity else {
                    return Err(query_error("not_logged_in", "login required", 401, false));
                };
                let account_owned = account.to_string();
                canonical.push(json!(account_owned));
                bound.push(BoundValue::Text(Some(account_owned)));
            }
            SqlParamType::Int
            | SqlParamType::Float
            | SqlParamType::Bool
            | SqlParamType::String
            | SqlParamType::Datetime => {
                let raw_input = raw_query
                    .get(&def.name)
                    .map(String::as_str)
                    .filter(|s| !s.is_empty());
                let effective =
                    raw_input.or_else(|| def.default_value.as_deref().filter(|s| !s.is_empty()));
                if let Some(raw_val) = effective {
                    let (bval, cval) = parse_typed_arg(def, raw_val)?;
                    bound.push(bval);
                    canonical.push(cval);
                } else if def.required {
                    return Err(query_error(
                        "validation",
                        format!("missing required parameter '{}'", def.name),
                        400,
                        false,
                    ));
                } else {
                    let null_bound = match def.param_type {
                        SqlParamType::Int => BoundValue::Int(None),
                        SqlParamType::Float => BoundValue::Float(None),
                        SqlParamType::Bool => BoundValue::Bool(None),
                        SqlParamType::String => BoundValue::Text(None),
                        SqlParamType::Datetime => BoundValue::Datetime(None),
                        SqlParamType::CurrentUserId | SqlParamType::CurrentAccount => {
                            unreachable!("handled in outer match")
                        }
                    };
                    bound.push(null_bound);
                    canonical.push(Value::Null);
                }
            }
        }
    }

    Ok((bound, Value::Array(canonical)))
}

pub(crate) fn resolve_bound_params(
    defs: &[SqlParamDef],
    raw_query: &HashMap<String, String>,
    session: &Session,
) -> Result<(Vec<BoundValue>, Value)> {
    let identity = session
        .is_login()
        .then(|| (session.get_user_id(), session.get_account()));
    resolve_bound_params_with(defs, raw_query, identity)
}

fn map_exec_sqlx_error(err: sqlx::Error, expose_db_err: bool) -> BaseError {
    if let sqlx::Error::Database(db_err) = &err {
        let code = db_err.code();
        if code.as_deref() == Some("57014") {
            return query_error("timeout", "sql query execution timed out", 504, false);
        }
        if code.as_deref() == Some("25006") {
            return query_error(
                "sql_not_readonly",
                "sql query attempted a write in read-only transaction",
                400,
                false,
            );
        }
        if expose_db_err {
            return query_error(
                "sql_execution_error",
                format!("sql execution failed: {}", db_err.message()),
                400,
                false,
            );
        }
    }
    if expose_db_err {
        query_error(
            "sql_execution_error",
            format!("sql execution failed: {err}"),
            400,
            false,
        )
    } else {
        warn!(target: LOG_TARGET, error = %err, "sql query execution failed");
        query_error(
            "sql_execution_error",
            "sql query execution failed",
            500,
            true,
        )
    }
}

async fn execute_readonly_query(
    pool: &PgPool,
    normalized_sql: &str,
    spec: &SqlQuerySpec,
    bound: &[BoundValue],
    expose_db_err: bool,
) -> Result<(usize, Value)> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| map_exec_sqlx_error(e, expose_db_err))?;

    sqlx::query("SET LOCAL transaction_read_only = on")
        .execute(&mut *tx)
        .await
        .map_err(|e| map_exec_sqlx_error(e, expose_db_err))?;

    let timeout_ms = spec.timeout_ms.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS);
    sqlx::query("SELECT set_config('statement_timeout', $1, true)")
        .bind(format!("{timeout_ms}ms"))
        .execute(&mut *tx)
        .await
        .map_err(|e| map_exec_sqlx_error(e, expose_db_err))?;

    let limit_idx = bound.len() + 1;
    let limit_rows = match spec.result_mode {
        SqlResultMode::Single => 1_i64,
        SqlResultMode::List => spec.max_rows.clamp(MIN_MAX_ROWS, MAX_MAX_ROWS),
    };
    let wrapped =
        format!("SELECT to_jsonb(_tibba_q) FROM ({normalized_sql}) AS _tibba_q LIMIT ${limit_idx}");

    // normalized_sql 已通过 validate_readonly_sql 词法校验，且当前事务已设 transaction_read_only = on
    let mut query = sqlx::query_as::<Postgres, (SqlxJson<Value>,)>(sqlx::AssertSqlSafe(wrapped));
    for val in bound {
        query = match val {
            BoundValue::Int(v) => query.bind(*v),
            BoundValue::Float(v) => query.bind(*v),
            BoundValue::Bool(v) => query.bind(*v),
            BoundValue::Text(v) => query.bind(v.as_deref()),
            BoundValue::Datetime(v) => query.bind(*v),
        };
    }
    query = query.bind(limit_rows);

    let rows = query
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| map_exec_sqlx_error(e, expose_db_err))?;

    tx.commit()
        .await
        .map_err(|e| map_exec_sqlx_error(e, expose_db_err))?;

    let row_count = rows.len();
    let result = match spec.result_mode {
        SqlResultMode::List => Value::Array(rows.into_iter().map(|r| r.0.0).collect()),
        SqlResultMode::Single => rows.into_iter().next().map_or(Value::Null, |r| r.0.0),
    };
    Ok((row_count, result))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedQueryResult {
    result: Value,
}

#[derive(Clone)]
struct QueryExecState {
    pool: &'static PgPool,
    cache: &'static RedisCache,
    configs: &'static ConfigStore,
}

/// 执行已配置并处于生效窗内的只读 SQL 查询接口。
#[utoipa::path(
    get,
    path = "/queries/{group}/{name}",
    tag = "query",
    params(
        ("group" = String, Path, description = "SQL 查询分组名"),
        ("name" = String, Path, description = "SQL 查询接口名")
    ),
    responses(
        (status = 200, description = "查询结果（list 模式为 JSON 数组，single 模式为对象或 null）"),
        (status = 400, description = "参数校验或查询配置非法"),
        (status = 401, description = "需要登录"),
        (status = 403, description = "无权限访问该查询"),
        (status = 404, description = "查询不存在、已禁用或不在生效时间窗内")
    )
)]
async fn execute_named_query(
    State(state): State<QueryExecState>,
    OptionalAppClient(app_client): OptionalAppClient,
    session: Session,
    Path((group, name)): Path<(String, String)>,
    Query(raw_query): Query<HashMap<String, String>>,
) -> JsonResult<Value> {
    validate_query_group(&group)?;
    validate_query_name(&name)?;
    let full_key = format_query_key(&group, &name);
    let cfg = state
        .configs
        .get_effective(SQL_QUERY_CATEGORY, &full_key)
        .await?
        .ok_or_else(|| {
            query_error(
                "query_not_found",
                format!("sql query not found or inactive: {full_key}"),
                404,
                false,
            )
        })?;

    let spec: SqlQuerySpec = serde_json::from_value(cfg.data).map_err(|e| {
        query_error(
            "stored_json",
            format!("invalid stored sql_query spec: {e}"),
            500,
            true,
        )
    })?;

    // 先鉴权，避免未授权用户探测参数名与配置细节
    check_query_caller_access(&spec, app_client.as_ref(), &group, &name, &session)?;

    let normalized_sql = validate_sql_query_spec(&spec)?;
    let (bound, canonical_params) = if let Some(ref app) = app_client {
        let app_account = format!("app:{}", app.app_id);
        resolve_bound_params_with(&spec.params, &raw_query, Some((0, app_account.as_str())))?
    } else {
        resolve_bound_params(&spec.params, &raw_query, &session)?
    };

    if spec.cache_ttl_secs > 0 {
        let digest = sha256(format!("{}\0{}", cfg.modified, canonical_params).as_bytes());
        let cache_key = format!("qres:{full_key}:{digest}");
        if let Ok(Some(cached)) = state
            .cache
            .get_struct::<CachedQueryResult>(&cache_key)
            .await
        {
            return Ok(Json(cached.result));
        }
        let (_, result) =
            execute_readonly_query(state.pool, &normalized_sql, &spec, &bound, false).await?;
        let _ = state
            .cache
            .set_struct(
                &cache_key,
                &CachedQueryResult {
                    result: result.clone(),
                },
                Some(Duration::from_secs(spec.cache_ttl_secs)),
            )
            .await;
        return Ok(Json(result));
    }

    let (_, result) =
        execute_readonly_query(state.pool, &normalized_sql, &spec, &bound, false).await?;
    Ok(Json(result))
}

/// 构建运行期 SQL 查询执行路由（挂载于 `/queries`）。
pub fn new_query_exec_router(
    pool: &'static PgPool,
    cache: &'static RedisCache,
    configs: &'static ConfigStore,
) -> Router {
    let state = QueryExecState {
        pool,
        cache,
        configs,
    };
    Router::new()
        .route("/{group}/{name}", get(execute_named_query))
        .with_state(state)
}

#[derive(Clone)]
struct SqlQueryAdminState {
    pool: &'static PgPool,
    configs: &'static ConfigStore,
}

/// 管理端展示的单条 SQL 查询接口记录。
#[derive(Debug, Serialize, ToSchema)]
pub struct SqlQueryAdminItem {
    pub id: i64,
    pub status: i16,
    pub group: String,
    pub name: String,
    pub description: String,
    pub effective_start_time: String,
    pub effective_end_time: String,
    pub created: String,
    pub modified: String,
    pub spec: Option<SqlQuerySpec>,
    #[schema(value_type = Object)]
    pub raw_data: Value,
}

impl From<Configuration> for SqlQueryAdminItem {
    fn from(cfg: Configuration) -> Self {
        let spec = serde_json::from_value::<SqlQuerySpec>(cfg.data.clone()).ok();
        let (group, name) = split_query_key(&cfg.name);
        Self {
            id: cfg.id,
            status: cfg.status,
            group,
            name,
            description: cfg.description,
            effective_start_time: cfg.effective_start_time,
            effective_end_time: cfg.effective_end_time,
            created: cfg.created,
            modified: cfg.modified,
            spec,
            raw_data: cfg.data,
        }
    }
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
struct CreateSqlQueryBody {
    #[validate(length(min = 1, max = 32))]
    group: String,
    #[validate(length(min = 1, max = 64))]
    name: String,
    #[serde(default)]
    #[validate(length(max = 255))]
    description: String,
    status: Option<i16>,
    effective_start_time: Option<String>,
    effective_end_time: Option<String>,
    spec: SqlQuerySpec,
}

#[derive(Debug, Serialize, ToSchema)]
struct CreateSqlQueryResp {
    id: u64,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
struct UpdateSqlQueryBody {
    #[validate(length(max = 255))]
    description: Option<String>,
    status: Option<i16>,
    effective_start_time: Option<String>,
    effective_end_time: Option<String>,
    spec: Option<SqlQuerySpec>,
}

#[derive(Debug, Deserialize, ToSchema)]
struct TestSqlQueryBody {
    spec: SqlQuerySpec,
    #[serde(default)]
    args: HashMap<String, String>,
}

#[derive(Debug, Serialize, ToSchema)]
struct TestSqlQueryResp {
    elapsed_ms: u64,
    row_count: usize,
    #[schema(value_type = Object)]
    result: Value,
}

/// SQL 查询分组条目。
#[derive(Debug, Serialize, ToSchema)]
pub struct SqlQueryGroupItem {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub query_count: usize,
    pub created: String,
    pub modified: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
struct CreateSqlQueryGroupBody {
    #[validate(length(min = 1, max = 32))]
    name: String,
    #[serde(default)]
    #[validate(length(max = 255))]
    description: String,
}

#[allow(clippy::too_many_arguments)]
async fn log_sql_query_audit(
    pool: &PgPool,
    session: &Session,
    action: &str,
    target_type: &str,
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
                .with_target(target_type, target_id)
                .with_request(request_id.as_str(), ip.to_string(), user_agent_of(headers))
                .with_detail(detail),
        )
        .await;
}

/// 列出全部已注册的 SQL 查询分组（`su` / `admin`）。
#[utoipa::path(
    get,
    path = "/sql-queries/groups",
    tag = "sql_query",
    responses(
        (status = 200, description = "SQL 查询分组列表", body = Vec<SqlQueryGroupItem>),
        (status = 403, description = "需要管理员或超级管理员权限")
    )
)]
async fn list_sql_query_groups(
    State(state): State<SqlQueryAdminState>,
    session: UserSession,
) -> JsonResult<Vec<SqlQueryGroupItem>> {
    ensure_admin(&session)?;
    let model = ConfigurationModel::new();
    let (groups, queries) = tokio::try_join!(
        model.list_by_category(state.pool, SQL_QUERY_GROUP_CATEGORY),
        model.list_by_category(state.pool, SQL_QUERY_CATEGORY),
    )?;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for q in queries {
        let (group, _) = split_query_key(&q.name);
        *counts.entry(group).or_insert(0) += 1;
    }
    let result = groups
        .into_iter()
        .map(|g| {
            let query_count = counts.get(&g.name).copied().unwrap_or(0);
            SqlQueryGroupItem {
                id: g.id,
                name: g.name,
                description: g.description,
                query_count,
                created: g.created,
                modified: g.modified,
            }
        })
        .collect();
    Ok(Json(result))
}

/// 新建 SQL 查询分组（`su` / `admin`）。
#[utoipa::path(
    post,
    path = "/sql-queries/groups",
    tag = "sql_query",
    request_body = CreateSqlQueryGroupBody,
    responses(
        (status = 200, description = "创建分组成功", body = CreateSqlQueryResp),
        (status = 400, description = "分组名不合法"),
        (status = 403, description = "需要管理员或超级管理员权限"),
        (status = 409, description = "分组已存在")
    )
)]
async fn create_sql_query_group(
    State(state): State<SqlQueryAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(body): JsonParams<CreateSqlQueryGroupBody>,
) -> JsonResult<CreateSqlQueryResp> {
    ensure_admin(&session)?;
    validate_query_group(&body.name)?;
    let model = ConfigurationModel::new();
    if model
        .get_by_category_name(state.pool, SQL_QUERY_GROUP_CATEGORY, &body.name)
        .await?
        .is_some()
    {
        return Err(query_error(
            "group_exists",
            format!("sql query group '{}' already exists", body.name),
            409,
            false,
        ));
    }

    let insert_payload = json!({
        "category": SQL_QUERY_GROUP_CATEGORY,
        "name": &body.name,
        "data": json!({}),
        "description": body.description,
        "status": Status::Enabled as i16,
        "effective_start_time": default_start_time(),
        "effective_end_time": DEFAULT_END_TIME,
    });

    let id = model.insert(state.pool, insert_payload).await?;
    state
        .configs
        .invalidate(SQL_QUERY_GROUP_CATEGORY, &body.name)
        .await;

    log_sql_query_audit(
        state.pool,
        &session,
        "sql_query_group.create",
        "sql_query_group",
        &body.name,
        &request_id,
        ip,
        &headers,
        json!({ "id": id }),
    )
    .await;

    Ok(Json(CreateSqlQueryResp { id }))
}

/// 删除 SQL 查询分组（仅当分组下无任何 SQL 查询接口时允许删除，`su` / `admin`）。
#[utoipa::path(
    delete,
    path = "/sql-queries/groups/{id}",
    tag = "sql_query",
    params(("id" = u64, Path, description = "configurations 主键 ID")),
    responses(
        (status = 204, description = "删除成功"),
        (status = 403, description = "需要管理员或超级管理员权限"),
        (status = 404, description = "分组不存在"),
        (status = 409, description = "分组仍有关联接口，无法删除")
    )
)]
async fn delete_sql_query_group(
    State(state): State<SqlQueryAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Result<StatusCode> {
    ensure_admin(&session)?;
    let model = ConfigurationModel::new();
    let Some(existing) = model.get_by_id(state.pool, id).await? else {
        return Err(query_error(
            "group_not_found",
            "sql query group not found",
            404,
            false,
        ));
    };
    if existing.category != SQL_QUERY_GROUP_CATEGORY {
        return Err(query_error(
            "group_not_found",
            "configuration is not a sql_query_group item",
            404,
            false,
        ));
    }

    let queries = model
        .list_by_category(state.pool, SQL_QUERY_CATEGORY)
        .await?;
    if queries
        .into_iter()
        .any(|q| split_query_key(&q.name).0 == existing.name)
    {
        return Err(query_error(
            "group_in_use",
            format!(
                "sql query group '{}' is still referenced by queries",
                existing.name
            ),
            409,
            false,
        ));
    }

    model.delete_by_id(state.pool, id).await?;
    state
        .configs
        .invalidate(SQL_QUERY_GROUP_CATEGORY, &existing.name)
        .await;

    log_sql_query_audit(
        state.pool,
        &session,
        "sql_query_group.delete",
        "sql_query_group",
        &existing.name,
        &request_id,
        ip,
        &headers,
        json!({ "id": id }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 列出全部配置化 SQL 查询定义（`su` / `admin`）。
#[utoipa::path(
    get,
    path = "/sql-queries",
    tag = "sql_query",
    responses(
        (status = 200, description = "SQL 查询接口列表", body = Vec<SqlQueryAdminItem>),
        (status = 403, description = "需要管理员或超级管理员权限")
    )
)]
async fn list_sql_queries(
    State(state): State<SqlQueryAdminState>,
    session: UserSession,
) -> JsonResult<Vec<SqlQueryAdminItem>> {
    ensure_admin(&session)?;
    let rows = ConfigurationModel::new()
        .list_by_category(state.pool, SQL_QUERY_CATEGORY)
        .await?;
    Ok(Json(
        rows.into_iter().map(SqlQueryAdminItem::from).collect(),
    ))
}

/// 新建配置化 SQL 查询接口（仅限 `su`，所选分组须已存在）。
#[utoipa::path(
    post,
    path = "/sql-queries",
    tag = "sql_query",
    request_body = CreateSqlQueryBody,
    responses(
        (status = 200, description = "创建成功", body = CreateSqlQueryResp),
        (status = 400, description = "分组不存在或 SQL / 参数定义非法"),
        (status = 403, description = "仅限超级管理员")
    )
)]
async fn create_sql_query(
    State(state): State<SqlQueryAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonParams(mut body): JsonParams<CreateSqlQueryBody>,
) -> JsonResult<CreateSqlQueryResp> {
    ensure_super_admin(&session)?;
    validate_query_group(&body.group)?;
    validate_query_name(&body.name)?;
    let model = ConfigurationModel::new();
    let Some(group_cfg) = model
        .get_by_category_name(state.pool, SQL_QUERY_GROUP_CATEGORY, &body.group)
        .await?
    else {
        return Err(query_error(
            "group_not_found",
            format!("sql query group '{}' does not exist", body.group),
            400,
            false,
        ));
    };
    if group_cfg.status != Status::Enabled as i16 {
        return Err(query_error(
            "group_not_found",
            format!("sql query group '{}' is disabled", body.group),
            400,
            false,
        ));
    }

    let full_key = format_query_key(&body.group, &body.name);
    let normalized_sql = validate_sql_query_spec(&body.spec)?;
    body.spec.sql = normalized_sql;

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

    let data = serde_json::to_value(&body.spec).map_err(|e| {
        query_error(
            "invalid_spec",
            format!("serialize spec failed: {e}"),
            400,
            false,
        )
    })?;

    let insert_payload = json!({
        "category": SQL_QUERY_CATEGORY,
        "name": &full_key,
        "data": data,
        "description": body.description,
        "status": body.status.unwrap_or(Status::Enabled as i16),
        "effective_start_time": start_str,
        "effective_end_time": end_str,
    });

    let id = model.insert(state.pool, insert_payload).await?;
    state
        .configs
        .invalidate(SQL_QUERY_CATEGORY, &full_key)
        .await;

    log_sql_query_audit(
        state.pool,
        &session,
        "sql_query.create",
        "sql_query",
        &full_key,
        &request_id,
        ip,
        &headers,
        json!({ "id": id, "access": body.spec.access }),
    )
    .await;

    Ok(Json(CreateSqlQueryResp { id }))
}

/// 更新配置化 SQL 查询接口（仅限 `su`）。
#[utoipa::path(
    patch,
    path = "/sql-queries/{id}",
    tag = "sql_query",
    params(("id" = u64, Path, description = "configurations 主键 ID")),
    request_body = UpdateSqlQueryBody,
    responses(
        (status = 204, description = "更新成功"),
        (status = 400, description = "SQL 或参数定义非法"),
        (status = 403, description = "仅限超级管理员"),
        (status = 404, description = "记录不存在或非 sql_query 分类")
    )
)]
async fn update_sql_query(
    State(state): State<SqlQueryAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(id): Path<u64>,
    JsonParams(mut body): JsonParams<UpdateSqlQueryBody>,
) -> Result<StatusCode> {
    ensure_super_admin(&session)?;
    let model = ConfigurationModel::new();
    let Some(existing) = model.get_by_id(state.pool, id).await? else {
        return Err(query_error(
            "query_not_found",
            "sql query config not found",
            404,
            false,
        ));
    };
    if existing.category != SQL_QUERY_CATEGORY {
        return Err(query_error(
            "query_not_found",
            "configuration is not a sql_query item",
            404,
            false,
        ));
    }

    let data_val = if let Some(mut spec) = body.spec.take() {
        spec.sql = validate_sql_query_spec(&spec)?;
        Some(serde_json::to_value(&spec).map_err(|e| {
            query_error(
                "invalid_spec",
                format!("serialize spec failed: {e}"),
                400,
                false,
            )
        })?)
    } else {
        None
    };

    let mut update_map = serde_json::Map::new();
    if let Some(d) = data_val {
        update_map.insert("data".to_string(), d);
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
        .invalidate(SQL_QUERY_CATEGORY, &existing.name)
        .await;

    log_sql_query_audit(
        state.pool,
        &session,
        "sql_query.update",
        "sql_query",
        &existing.name,
        &request_id,
        ip,
        &headers,
        json!({ "id": id }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 软删除配置化 SQL 查询接口（仅限 `su`）。
#[utoipa::path(
    delete,
    path = "/sql-queries/{id}",
    tag = "sql_query",
    params(("id" = u64, Path, description = "configurations 主键 ID")),
    responses(
        (status = 204, description = "删除成功"),
        (status = 403, description = "仅限超级管理员"),
        (status = 404, description = "记录不存在或非 sql_query 分类")
    )
)]
async fn delete_sql_query(
    State(state): State<SqlQueryAdminState>,
    session: UserSession,
    request_id: RequestId,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Result<StatusCode> {
    ensure_super_admin(&session)?;
    let model = ConfigurationModel::new();
    let Some(existing) = model.get_by_id(state.pool, id).await? else {
        return Err(query_error(
            "query_not_found",
            "sql query config not found",
            404,
            false,
        ));
    };
    if existing.category != SQL_QUERY_CATEGORY {
        return Err(query_error(
            "query_not_found",
            "configuration is not a sql_query item",
            404,
            false,
        ));
    }

    model.delete_by_id(state.pool, id).await?;
    state
        .configs
        .invalidate(SQL_QUERY_CATEGORY, &existing.name)
        .await;

    log_sql_query_audit(
        state.pool,
        &session,
        "sql_query.delete",
        "sql_query",
        &existing.name,
        &request_id,
        ip,
        &headers,
        json!({ "id": id }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// 仅限 `su` 超管使用的在线带参试运行预览（不写库、同样强制只读事务 + 参数绑定 + 超时控制）。
#[utoipa::path(
    post,
    path = "/sql-queries/test",
    tag = "sql_query",
    request_body = TestSqlQueryBody,
    responses(
        (status = 200, description = "试运行成功，返回耗时与结果 JSON", body = TestSqlQueryResp),
        (status = 400, description = "SQL、参数或执行错误"),
        (status = 403, description = "仅限超级管理员"),
        (status = 504, description = "查询执行超时")
    )
)]
async fn test_sql_query(
    State(state): State<SqlQueryAdminState>,
    session: UserSession,
    Json(body): Json<TestSqlQueryBody>,
) -> JsonResult<TestSqlQueryResp> {
    ensure_super_admin(&session)?;
    let normalized_sql = validate_sql_query_spec(&body.spec)?;
    let (bound, _) = resolve_bound_params(&body.spec.params, &body.args, &session)?;

    let started = Instant::now();
    let (row_count, result) =
        execute_readonly_query(state.pool, &normalized_sql, &body.spec, &bound, true).await?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    Ok(Json(TestSqlQueryResp {
        elapsed_ms,
        row_count,
        result,
    }))
}

/// 构建配置化 SQL 查询管理端路由（挂载于 `/sql-queries`）。
pub fn new_sql_query_admin_router(pool: &'static PgPool, configs: &'static ConfigStore) -> Router {
    let state = SqlQueryAdminState { pool, configs };
    Router::new()
        .route("/", get(list_sql_queries).post(create_sql_query))
        .route(
            "/groups",
            get(list_sql_query_groups).post(create_sql_query_group),
        )
        .route("/groups/{id}", delete(delete_sql_query_group))
        .route("/{id}", patch(update_sql_query).delete(delete_sql_query))
        .route("/test", post(test_sql_query))
        .with_state(state)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        execute_named_query,
        list_sql_query_groups,
        create_sql_query_group,
        delete_sql_query_group,
        list_sql_queries,
        create_sql_query,
        update_sql_query,
        delete_sql_query,
        test_sql_query,
    ),
    components(schemas(
        SqlAccessLevel,
        SqlResultMode,
        SqlParamType,
        SqlParamDef,
        SqlQuerySpec,
        SqlQueryGroupItem,
        CreateSqlQueryGroupBody,
        SqlQueryAdminItem,
        CreateSqlQueryBody,
        CreateSqlQueryResp,
        UpdateSqlQueryBody,
        TestSqlQueryBody,
        TestSqlQueryResp,
    )),
    tags(
        (name = "query", description = "运行期配置化只读 SQL 查询执行接口"),
        (name = "sql_query", description = "配置化 SQL 查询接口及分组管理")
    )
)]
struct SqlQueryApiDoc;

/// 返回 `sql_query` 路由的 OpenAPI 文档片段。
pub fn openapi() -> utoipa::openapi::OpenApi {
    SqlQueryApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_readonly_sql_accepts_select_with_and_string_semicolons() {
        let sql = "-- leading comment\nSELECT id, 'a;b' AS sep, E'x\\';y' AS esc FROM users WHERE status = $1;";
        let normalized = validate_readonly_sql(sql, 1).expect("valid select");
        assert!(normalized.ends_with("= $1"));

        let cte = "/* header */ WITH recent AS (SELECT id FROM audit_logs WHERE user_id = $1) SELECT * FROM recent";
        assert!(validate_readonly_sql(cte, 1).is_ok());

        let nested_comment = "/* outer /* inner */ outer */ SELECT \"delete\", created FROM users";
        assert!(validate_readonly_sql(nested_comment, 0).is_ok());
    }

    #[test]
    fn validate_readonly_sql_rejects_writes_multistatement_and_bad_placeholders() {
        for bad in [
            "",
            "INSERT INTO users (account) VALUES ('x')",
            "UPDATE users SET status = 0",
            "DELETE FROM users",
            "DROP TABLE users",
            "WITH d AS (DELETE FROM users RETURNING id) SELECT * FROM d",
            "SELECT 1; SELECT 2",
            "SELECT 1; DROP TABLE users",
            "/* unclosed SELECT 1",
            "SELECT 'unclosed",
            "SELECT \"unclosed FROM users",
            "SELECT $$hello;world$$",
            "SELECT * FROM users WHERE id = $0",
            "SELECT * FROM users WHERE id = $2",
        ] {
            let err = validate_readonly_sql(bad, 1)
                .expect_err(&format!("expected failure for sql: {bad}"));
            assert_eq!(err.status(), 400);
            assert_eq!(err.sub_category(), Some("invalid_sql"));
        }
    }

    #[test]
    fn validate_sql_query_spec_checks_constraints_and_permissions() {
        let public_with_user = SqlQuerySpec {
            sql: "SELECT id FROM users WHERE id = $1".to_string(),
            params: vec![SqlParamDef {
                name: "uid".to_string(),
                param_type: SqlParamType::CurrentUserId,
                required: false,
                default_value: None,
                min: None,
                max: None,
            }],
            access: SqlAccessLevel::Public,
            permission_code: String::new(),
            result_mode: SqlResultMode::Single,
            timeout_ms: 3000,
            max_rows: 100,
            cache_ttl_secs: 0,
        };
        assert_eq!(
            validate_sql_query_spec(&public_with_user)
                .expect_err("public session param must fail")
                .status(),
            400
        );

        let wildcard_perm = SqlQuerySpec {
            sql: "SELECT 1".to_string(),
            params: vec![],
            access: SqlAccessLevel::Permission,
            permission_code: "*".to_string(),
            result_mode: SqlResultMode::Single,
            timeout_ms: 3000,
            max_rows: 100,
            cache_ttl_secs: 0,
        };
        assert_eq!(
            validate_sql_query_spec(&wildcard_perm)
                .expect_err("wildcard perm must fail")
                .sub_category(),
            Some("wildcard_reserved")
        );

        let bad_bounds = SqlQuerySpec {
            sql: "SELECT $1".to_string(),
            params: vec![SqlParamDef {
                name: "limit".to_string(),
                param_type: SqlParamType::Int,
                required: false,
                default_value: Some("5".to_string()),
                min: Some(10.0),
                max: Some(20.0),
            }],
            access: SqlAccessLevel::Admin,
            permission_code: String::new(),
            result_mode: SqlResultMode::List,
            timeout_ms: 3000,
            max_rows: 100,
            cache_ttl_secs: 0,
        };
        assert_eq!(
            validate_sql_query_spec(&bad_bounds)
                .expect_err("default below min must fail")
                .status(),
            400
        );
    }

    #[test]
    fn resolve_bound_params_handles_types_defaults_bounds_and_session() {
        let defs = vec![
            SqlParamDef {
                name: "uid".to_string(),
                param_type: SqlParamType::CurrentUserId,
                required: true,
                default_value: None,
                min: None,
                max: None,
            },
            SqlParamDef {
                name: "days".to_string(),
                param_type: SqlParamType::Int,
                required: false,
                default_value: Some("7".to_string()),
                min: Some(1.0),
                max: Some(90.0),
            },
            SqlParamDef {
                name: "since".to_string(),
                param_type: SqlParamType::Datetime,
                required: false,
                default_value: None,
                min: None,
                max: None,
            },
            SqlParamDef {
                name: "active".to_string(),
                param_type: SqlParamType::Bool,
                required: false,
                default_value: Some("true".to_string()),
                min: None,
                max: None,
            },
        ];

        let err = resolve_bound_params_with(&defs, &HashMap::new(), None)
            .expect_err("session param must reject guest");
        assert_eq!(err.status(), 401);

        let app_err = resolve_bound_params_with(&defs, &HashMap::new(), Some((0, "app:svc_a")))
            .expect_err("current_user_id must reject app session");
        assert_eq!(app_err.status(), 403);
        assert_eq!(app_err.sub_category(), Some("app_session_forbidden"));

        let (bound, canonical) =
            resolve_bound_params_with(&defs, &HashMap::new(), Some((42, "alice")))
                .expect("defaults resolve");
        assert_eq!(
            bound,
            vec![
                BoundValue::Int(Some(42)),
                BoundValue::Int(Some(7)),
                BoundValue::Datetime(None),
                BoundValue::Bool(Some(true)),
            ]
        );
        assert_eq!(canonical, json!([42, 7, null, true]));

        let mut bad_range = HashMap::new();
        bad_range.insert("days".to_string(), "120".to_string());
        assert_eq!(
            resolve_bound_params_with(&defs, &bad_range, Some((42, "alice")))
                .expect_err("out of bounds int must fail")
                .status(),
            400
        );
    }

    #[test]
    fn check_query_access_enforces_levels() {
        let mut spec = SqlQuerySpec {
            sql: "SELECT 1".to_string(),
            params: vec![],
            access: SqlAccessLevel::Public,
            permission_code: String::new(),
            result_mode: SqlResultMode::Single,
            timeout_ms: 3000,
            max_rows: 100,
            cache_ttl_secs: 0,
        };
        assert!(check_query_access_parts(&spec, false, false, false).is_ok());

        spec.access = SqlAccessLevel::Login;
        assert_eq!(
            check_query_access_parts(&spec, false, false, false)
                .expect_err("401")
                .status(),
            401
        );
        assert!(check_query_access_parts(&spec, true, false, false).is_ok());

        spec.access = SqlAccessLevel::Admin;
        assert_eq!(
            check_query_access_parts(&spec, true, false, false)
                .expect_err("403")
                .status(),
            403
        );
        assert!(check_query_access_parts(&spec, true, true, false).is_ok());

        spec.access = SqlAccessLevel::Permission;
        spec.permission_code = "report:read".to_string();
        assert!(check_query_access_parts(&spec, true, false, true).is_ok());
        assert_eq!(
            check_query_access_parts(&spec, true, false, false)
                .expect_err("403")
                .status(),
            403
        );
        assert!(check_query_access_parts(&spec, true, true, false).is_ok());
    }

    #[test]
    fn query_group_and_name_validation_and_key_split() {
        assert!(validate_query_group("report").is_ok());
        assert!(validate_query_group("order-stats_01").is_ok());
        assert!(validate_query_group("").is_err());
        assert!(validate_query_group("bad/group").is_err());
        assert!(validate_query_group(&"g".repeat(33)).is_err());

        assert!(validate_query_name("recent_orders").is_ok());
        assert!(validate_query_name("").is_err());
        assert!(validate_query_name("bad/name").is_err());
        assert!(validate_query_name(&"n".repeat(65)).is_err());

        let key = format_query_key("report", "recent_orders");
        assert_eq!(key, "report/recent_orders");
        assert_eq!(
            split_query_key(&key),
            ("report".to_string(), "recent_orders".to_string())
        );
        assert_eq!(
            split_query_key("legacy_single_name"),
            ("default".to_string(), "legacy_single_name".to_string())
        );

        let su = vec!["su".to_string()];
        let admin = vec!["admin".to_string()];
        let viewer = vec!["viewer".to_string()];
        assert_eq!(
            check_role_allowed(false, &su, false)
                .expect_err("unauthenticated must fail")
                .status(),
            401
        );
        assert!(check_role_allowed(true, &su, true).is_ok());
        assert!(check_role_allowed(true, &admin, false).is_ok());
        assert_eq!(
            check_role_allowed(true, &admin, true)
                .expect_err("admin cannot perform su-only action")
                .status(),
            403
        );
        assert_eq!(
            check_role_allowed(true, &viewer, false)
                .expect_err("non-admin cannot access group admin")
                .status(),
            403
        );
    }

    #[test]
    fn default_effective_time_bounds_parse_cleanly() {
        assert!(parse_primitive_datetime(&default_start_time()).is_ok());
        assert!(parse_primitive_datetime(DEFAULT_END_TIME).is_ok());
    }

    #[test]
    fn check_app_query_access_allows_exact_wildcard_and_public_only() {
        let admin_spec = SqlQuerySpec {
            sql: "SELECT 1".to_string(),
            params: vec![],
            access: SqlAccessLevel::Admin,
            permission_code: String::new(),
            result_mode: SqlResultMode::Single,
            timeout_ms: 3000,
            max_rows: 100,
            cache_ttl_secs: 0,
        };
        let app = crate::AppClientIdentity {
            app_id: "zedis".to_string(),
            allowed_queries: vec!["group1/query1".to_string(), "report/*".to_string()],
        };

        assert!(check_app_query_access(&admin_spec, &app, "group1", "query1").is_ok());
        assert!(check_app_query_access(&admin_spec, &app, "report", "any_new_query").is_ok());

        let denied = check_app_query_access(&admin_spec, &app, "group1", "query2")
            .expect_err("unauthorized exact query must fail");
        assert_eq!(denied.status(), 403);
        assert_eq!(denied.sub_category(), Some("permission_denied"));

        let denied_group = check_app_query_access(&admin_spec, &app, "group2", "query1")
            .expect_err("unauthorized group must fail");
        assert_eq!(denied_group.status(), 403);

        let public_spec = SqlQuerySpec {
            access: SqlAccessLevel::Public,
            ..admin_spec
        };
        assert!(check_app_query_access(&public_spec, &app, "other", "ping").is_ok());
    }
}
