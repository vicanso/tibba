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

//! 行级多租户原语（可选一等能力）。
//!
//! ## 隔离模型
//! 共享表 + `tenant_id` 列；所有读写恒按租户过滤。单行操作建议
//! `WHERE id = $1 AND tenant_id = $2` 做纵深防御（防 IDOR）。
//!
//! ## 信任边界
//! | 来源 | 可信度 | 用途 |
//! |------|--------|------|
//! | 扩展中的 [`TenantId`]（鉴权中间件注入） | 高 | **生产默认** |
//! | `X-Tenant-Id` 请求头 | 低 | 仅开发 / 显式信任代理 |
//!
//! [`inject_tenant_from_user_id`] 在登录 Session 上派生租户并写入扩展，使
//! [`TenantId`] 提取器**忽略**客户端可伪造的头。
//!
//! ## SQL 片段
//! 见 [`sql`] 模块：供 `QueryBuilder::push` 或文档对齐；完整 `query` 字面量仍须
//! 满足 sqlx 0.9 `SqlSafeStr`（用静态字符串）。

use axum::extract::{FromRequestParts, Request};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use snafu::Snafu;
use tibba_error::Error as BaseError;
use tibba_session::UserSession;

/// 解析租户的请求头名（小写，HTTP 头大小写不敏感）。
const HEADER_TENANT_ID: &str = "x-tenant-id";
/// 租户标识最大长度。
const MAX_TENANT_ID_LEN: usize = 64;

/// 本 crate 内部错误，统一转换为 [`tibba_error::Error`]（均为 400）。
#[derive(Debug, Snafu)]
pub enum Error {
    /// 请求既无 `TenantId` 扩展也无 `X-Tenant-Id` 头。
    #[snafu(display("missing tenant id (set the X-Tenant-Id header)"))]
    Missing,
    /// 租户标识非法（空 / 超长 / 含非法字符）。
    #[snafu(display("invalid tenant id: {reason}"))]
    Invalid { reason: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Missing => BaseError::new("missing tenant id (set the X-Tenant-Id header)")
                .with_sub_category("tenant_missing")
                .with_status(400),
            Error::Invalid { reason } => BaseError::new(format!("invalid tenant id: {reason}"))
                .with_sub_category("tenant_invalid")
                .with_status(400),
        };
        err.with_category("tenant")
    }
}

/// 行级多租户 SQL 片段（列名约定 `tenant_id`）。
///
/// sqlx 0.9 的 `query(...)` 只接受 `'static` 字面量；动态拼接请用 `QueryBuilder`。
pub mod sql {
    /// 多租户列名约定。
    pub const COLUMN: &str = "tenant_id";

    /// `QueryBuilder`：`qb.push(WHERE_TENANT_EQ); qb.push_bind(tenant.as_str());`
    pub const WHERE_TENANT_EQ: &str = " WHERE tenant_id = ";

    /// `QueryBuilder`：追加租户过滤。
    pub const AND_TENANT_EQ: &str = " AND tenant_id = ";

    /// 静态 query 用：`$1` = 行 id，`$2` = tenant_id（纵深防御）。
    pub const WHERE_ID_AND_TENANT: &str = " WHERE id = $1 AND tenant_id = $2";
}

/// 租户标识。行级多租户的隔离键：每条租户数据带 `tenant_id`，查询恒按它过滤。
///
/// 仅能经 [`TenantId::parse`]（或提取器）创建，保证内部值已通过字符集与长度校验，
/// 可安全用作 SQL 绑定值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantId(String);

impl TenantId {
    /// 借出租户标识字符串（用于 SQL `bind`）。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 取出内部字符串。
    pub fn into_inner(self) -> String {
        self.0
    }

    /// 由已登录用户 id 派生默认租户键：`u{user_id}`。
    ///
    /// demo / 单租户-per-user 场景足够；SaaS 应改为成员表或 JWT claim。
    pub fn from_user_id(user_id: i64) -> Result<Self, Error> {
        Self::parse(format!("u{user_id}"))
    }

    /// 校验并构造：非空、长度 ≤ 64、仅 ASCII 字母数字与 `-` `_`。
    /// 字符集收敛既防脏数据，也让租户标识能安全用于日志 / 键名等场景。
    pub fn parse(raw: impl Into<String>) -> Result<Self, Error> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(Error::Invalid {
                reason: "empty".to_string(),
            });
        }
        if raw.len() > MAX_TENANT_ID_LEN {
            return Err(Error::Invalid {
                reason: format!("too long (max {MAX_TENANT_ID_LEN})"),
            });
        }
        if !raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Error::Invalid {
                reason: "only [A-Za-z0-9_-] allowed".to_string(),
            });
        }
        Ok(Self(raw))
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 从请求解析租户：优先取已注入的 [`TenantId`] 扩展，否则取 `X-Tenant-Id` 头；
/// 都没有则 400。值经 [`TenantId::parse`] 校验。
impl<S: Sync> FromRequestParts<S> for TenantId {
    type Rejection = BaseError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        // 1. 上游注入的扩展优先（鉴权层从 JWT/session 解出租户后放入扩展即可被采用，
        //    本 crate 因此无需依赖 session / jwt）
        if let Some(tenant) = parts.extensions.get::<TenantId>() {
            return Ok(tenant.clone());
        }
        // 2. X-Tenant-Id 头
        let Some(raw) = parts
            .headers
            .get(HEADER_TENANT_ID)
            .and_then(|v| v.to_str().ok())
        else {
            return Err(Error::Missing.into());
        };
        Ok(TenantId::parse(raw)?)
    }
}

/// 可选租户：缺失返回 `None`，存在但非法返回 400。
/// 用于「登录可不带租户、带了才按租户隔离」的端点。
pub struct OptionalTenantId(pub Option<TenantId>);

impl<S: Sync> FromRequestParts<S> for OptionalTenantId {
    type Rejection = BaseError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        if let Some(tenant) = parts.extensions.get::<TenantId>() {
            return Ok(Self(Some(tenant.clone())));
        }
        let Some(raw) = parts
            .headers
            .get(HEADER_TENANT_ID)
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(Self(None));
        };
        Ok(Self(Some(TenantId::parse(raw)?)))
    }
}

type ResultResponse = std::result::Result<Response, BaseError>;

/// 中间件：要求登录，从 Session 用户 id 派生租户并注入扩展。
///
/// 注入后 [`TenantId`] 提取器优先读扩展，**不再信任**客户端 `X-Tenant-Id`。
/// 挂在需要租户隔离的路由子树上即可（不必全局启用）。
///
/// ```ignore
/// Router::new()
///     .route("/notes", get(list))
///     .layer(from_fn(inject_tenant_from_user_id));
/// ```
pub async fn inject_tenant_from_user_id(req: Request, next: Next) -> ResultResponse {
    let (mut parts, body) = req.into_parts();
    let user = UserSession::from_request_parts(&mut parts, &()).await?;
    let tenant = TenantId::from_user_id(user.get_user_id())?;
    parts.extensions.insert(tenant);
    Ok(next.run(Request::from_parts(parts, body)).await)
}

/// 将任意已校验的 [`TenantId`] 写入扩展（供 JWT claim / 成员表解析后调用）。
///
/// 与 [`inject_tenant_from_user_id`] 相同：扩展优先于请求头。
pub fn insert_tenant_extension(parts: &mut Parts, tenant: TenantId) {
    parts.extensions.insert(tenant);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_valid() {
        assert_eq!(TenantId::parse("acme-01_x").unwrap().as_str(), "acme-01_x");
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(matches!(
            TenantId::parse("").unwrap_err(),
            Error::Invalid { .. }
        ));
    }

    #[test]
    fn parse_rejects_too_long() {
        let long = "a".repeat(MAX_TENANT_ID_LEN + 1);
        assert!(matches!(
            TenantId::parse(long).unwrap_err(),
            Error::Invalid { .. }
        ));
    }

    #[test]
    fn parse_rejects_bad_chars() {
        // 空格 / 斜杠 / 中文等非法
        for bad in ["a b", "a/b", "../etc", "租户", "a;b"] {
            assert!(
                matches!(TenantId::parse(bad).unwrap_err(), Error::Invalid { .. }),
                "应拒绝: {bad:?}"
            );
        }
    }

    #[test]
    fn from_user_id_shape() {
        assert_eq!(TenantId::from_user_id(42).unwrap().as_str(), "u42");
    }

    #[test]
    fn sql_fragments_mention_tenant_column() {
        assert!(sql::WHERE_TENANT_EQ.contains(sql::COLUMN));
        assert!(sql::AND_TENANT_EQ.contains(sql::COLUMN));
        assert!(sql::WHERE_ID_AND_TENANT.contains("id = $1"));
        assert!(sql::WHERE_ID_AND_TENANT.contains("tenant_id = $2"));
    }
}
