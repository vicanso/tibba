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

//! 行级多租户原语：从请求中解析[租户标识](TenantId)的 axum 提取器。
//!
//! 隔离模型为**行级**——共享表，每张多租户表带 `tenant_id` 列，所有读写恒按它过滤。
//! 本 crate 只提供「拿到当前租户」这一步；具体表的 scoping 由业务在 SQL 里
//! `WHERE tenant_id = $T` 完成（增删改建议带 `id = $1 AND tenant_id = $2` 做纵深防御，
//! 即便猜到别的租户的行 id 也无法越权读写）。不改动任何现有表。
//!
//! ## 租户来源（按优先级）
//! 1. 请求扩展中的 [`TenantId`]：上游中间件（如鉴权层从 JWT claim / session 解出租户后
//!    `req.extensions_mut().insert(TenantId)`）注入，最可信；
//! 2. `X-Tenant-Id` 请求头。
//!
//! 两者都没有时 [`TenantId`] 提取器返回 400；需「可选」语义用 [`OptionalTenantId`]。
//!
//! ## 用法
//! ```ignore
//! async fn list_notes(tenant: TenantId) -> Result<Json<Vec<Note>>> {
//!     let rows = sqlx::query_as("SELECT ... FROM notes WHERE tenant_id = $1")
//!         .bind(tenant.as_str())
//!         .fetch_all(pool).await?;
//!     // ...
//! }
//! ```

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use snafu::Snafu;
use tibba_error::Error as BaseError;

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
}
