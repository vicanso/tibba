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

//! JWT claims 结构 + axum 用户身份提取器。

use crate::{LOG_TARGET, try_global_signer};
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};
use tibba_error::Error as BaseError;
use tracing::debug;

/// HS256 JWT 的标准 claims + 业务字段。
///
/// 标准字段（RFC 7519 子集）：
/// - `sub`: subject（本系统 = user_id i64）
/// - `iss`: issuer，用于多服务区分
/// - `iat`: issued at（unix seconds）
/// - `exp`: expiration（unix seconds）
/// - `jti`: JWT ID（UUID），便于审计 / 黑名单
///
/// 业务字段：
/// - `account`: 用户账号（前端展示用，避免再查 DB）
/// - `roles`: 角色列表
/// - `permissions`: 权限码列表（前端按钮可见性 + 后端守卫）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub iss: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    pub account: String,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// axum extractor —— 从 `Authorization: Bearer <jwt>` 解出已鉴权用户。
///
/// 拒绝路径：
/// - 全局 signer 未初始化（`[jwt]` 未配 secret）→ 503
/// - header 缺失 / 不是 Bearer 格式 → 401 "missing bearer token"
/// - JWT 验签 / 过期 / 篡改 → 401 "jwt verify failed: ..."
#[derive(Debug, Clone)]
pub struct JwtUser {
    pub user_id: i64,
    pub account: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    /// 给业务侧用于撤销 / 审计的 JWT ID
    pub jti: String,
}

impl JwtUser {
    /// 与 `tibba_session::permission_grants` 对齐的权限通配判断 —— 不引入跨 crate 依赖，
    /// 这里手写一份小匹配，规则与 session 路径完全一致：
    /// - `"*"` 命中任意
    /// - `"resource:*"` 命中 `"resource:..."`
    /// - 其余精确匹配
    pub fn has_permission(&self, required: &str) -> bool {
        if required.is_empty() {
            return false;
        }
        for g in &self.permissions {
            if g == "*" || g == required {
                return true;
            }
            if let Some(prefix) = g.strip_suffix(":*") {
                let needle = format!("{prefix}:");
                if required.starts_with(&needle) && required.len() > needle.len() {
                    return true;
                }
            }
        }
        false
    }
}

impl From<Claims> for JwtUser {
    fn from(c: Claims) -> Self {
        Self {
            user_id: c.sub,
            account: c.account,
            roles: c.roles,
            permissions: c.permissions,
            jti: c.jti,
        }
    }
}

impl<S> FromRequestParts<S> for JwtUser
where
    S: Sync,
{
    type Rejection = BaseError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        // 全局 signer 未配置时 503，明确告诉客户端 JWT 端点未启用
        let signer = try_global_signer().ok_or_else(|| {
            BaseError::new("jwt signer not initialized; [jwt] secret missing")
                .with_category("jwt")
                .with_sub_category("not_configured")
                .with_status(503)
                .with_exception(true)
        })?;

        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                BaseError::new("missing bearer token")
                    .with_category("jwt")
                    .with_sub_category("missing_bearer")
                    .with_status(401)
                    .with_exception(false)
            })?;

        let claims = signer.verify_access(token).inspect_err(|e| {
            debug!(target: LOG_TARGET, error = %e, "jwt verify failed");
        })?;
        Ok(JwtUser::from(claims))
    }
}
