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

//! `audit_logs` 表的 CRUD 接口。
//!
//! 设计原则（Phase II.A Q3 选择 ii）：handler 关键操作显式调用 `log(...)`，
//! 而不是用全局中间件包打所有请求 —— 这样审计行只覆盖真正有业务语义的事件，
//! 不会被 OPTIONS / preflight / health check 等高频噪音淹没。
//!
//! ## 调用示例
//!
//! ```ignore
//! use tibba_model_builtin::{AuditLogModel, AuditLogParams};
//!
//! // 登录成功后
//! let _ = AuditLogModel::new()
//!     .log(pool, AuditLogParams::new("user.login")
//!         .with_user(user.id)
//!         .with_target("user", user.id.to_string())
//!         .with_request(&request_id, &client_ip, user_agent))
//!     .await;
//! ```
//!
//! 故意忽略 log 的 Result —— 审计失败**不应阻断业务**。调用方应该 `let _ = ...await;`
//! 或仅记入 warn 日志。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use tibba_model::{Error, SqlxSnafu, format_datetime};
use time::PrimitiveDateTime;

type Result<T> = std::result::Result<T, Error>;

const MAX_TARGET_TYPE_LEN: usize = 64;
const MAX_TARGET_ID_LEN: usize = 64;
const MAX_REQUEST_ID_LEN: usize = 128;
const MAX_IP_LEN: usize = 64;
const MAX_USER_AGENT_LEN: usize = 255;
const MAX_ACTION_LEN: usize = 64;

#[derive(FromRow)]
struct AuditLogSchema {
    id: i64,
    user_id: Option<i64>,
    action: String,
    target_type: String,
    target_id: String,
    detail: Json<Value>,
    request_id: String,
    ip: String,
    user_agent: String,
    created: PrimitiveDateTime,
}

/// 单条审计日志记录（对 admin 接口 / 列表查询暴露）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: i64,
    pub user_id: Option<i64>,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub detail: Value,
    pub request_id: String,
    pub ip: String,
    pub user_agent: String,
    pub created: String,
}

impl From<AuditLogSchema> for AuditLog {
    fn from(s: AuditLogSchema) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            action: s.action,
            target_type: s.target_type,
            target_id: s.target_id,
            detail: s.detail.0,
            request_id: s.request_id,
            ip: s.ip,
            user_agent: s.user_agent,
            created: format_datetime(s.created),
        }
    }
}

/// 写入一条审计行的入参。
///
/// 字段约束：
/// - `action` 必填；约定 `"{resource}.{action}"` 形如 `"user.login"`
/// - 其它字段全部可空（空串 = 不填，不抛错）
/// - 字符串字段会在 `log()` 中被静默截断到列长度上限，防止 UA 等异常长输入打爆 DB
///
/// 用链式 setter 构造，单次 .log() 不复用（生命周期清晰）。
#[derive(Debug, Clone, Default)]
pub struct AuditLogParams {
    user_id: Option<i64>,
    action: String,
    target_type: String,
    target_id: String,
    detail: Option<Value>,
    request_id: String,
    ip: String,
    user_agent: String,
}

impl AuditLogParams {
    /// 起手必须给 action。其余字段按需链式补充。
    pub fn new(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            ..Self::default()
        }
    }

    /// 设置操作主体（已登录用户 id）。
    #[must_use]
    pub fn with_user(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// 设置操作目标（如 `("user", "12345")`）。
    #[must_use]
    pub fn with_target(
        mut self,
        target_type: impl Into<String>,
        target_id: impl Into<String>,
    ) -> Self {
        self.target_type = target_type.into();
        self.target_id = target_id.into();
        self
    }

    /// 一次性塞 request_id / ip / user_agent。handler 拿到这三个就一起塞，
    /// 避免每个字段单独 setter 的口水代码。
    #[must_use]
    pub fn with_request(
        mut self,
        request_id: impl Into<String>,
        ip: impl Into<String>,
        user_agent: impl Into<String>,
    ) -> Self {
        self.request_id = request_id.into();
        self.ip = ip.into();
        self.user_agent = user_agent.into();
        self
    }

    /// 自由结构化补充信息（前后值、provider、命中规则等）。
    #[must_use]
    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

#[derive(Default)]
pub struct AuditLogModel;

impl AuditLogModel {
    pub fn new() -> Self {
        Self
    }

    /// 写一条审计日志。
    /// **注意**：调用方应忽略 Err，审计失败不能影响业务请求。
    pub async fn log(&self, pool: &Pool<Postgres>, params: AuditLogParams) -> Result<i64> {
        let detail = params.detail.unwrap_or_else(|| Value::Object(Default::default()));
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO audit_logs
                 (user_id, action, target_type, target_id, detail,
                  request_id, ip, user_agent)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               RETURNING id"#,
        )
        .bind(params.user_id)
        .bind(truncate(&params.action, MAX_ACTION_LEN))
        .bind(truncate(&params.target_type, MAX_TARGET_TYPE_LEN))
        .bind(truncate(&params.target_id, MAX_TARGET_ID_LEN))
        .bind(Json(detail))
        .bind(truncate(&params.request_id, MAX_REQUEST_ID_LEN))
        .bind(truncate(&params.ip, MAX_IP_LEN))
        .bind(truncate(&params.user_agent, MAX_USER_AGENT_LEN))
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.0)
    }

    /// 按 user_id 倒序列出指定用户的审计史（admin 用）。
    pub async fn list_by_user(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        limit: i64,
    ) -> Result<Vec<AuditLog>> {
        let rows: Vec<AuditLogSchema> = sqlx::query_as(
            r#"SELECT id, user_id, action, target_type, target_id, detail,
                      request_id, ip, user_agent, created
               FROM audit_logs
               WHERE user_id = $1
               ORDER BY created DESC
               LIMIT $2"#,
        )
        .bind(user_id)
        .bind(limit.clamp(1, 1000))
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(AuditLog::from).collect())
    }

    /// 按 request_id 串完整请求链路（排障用）。
    pub async fn list_by_request(
        &self,
        pool: &Pool<Postgres>,
        request_id: &str,
    ) -> Result<Vec<AuditLog>> {
        let rows: Vec<AuditLogSchema> = sqlx::query_as(
            r#"SELECT id, user_id, action, target_type, target_id, detail,
                      request_id, ip, user_agent, created
               FROM audit_logs
               WHERE request_id = $1
               ORDER BY created ASC"#,
        )
        .bind(request_id)
        .fetch_all(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows.into_iter().map(AuditLog::from).collect())
    }
}

/// 按字符（而非字节）截断，避免砍坏 UTF-8。空串 / 短串直接克隆返回。
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}
