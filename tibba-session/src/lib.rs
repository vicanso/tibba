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

use snafu::Snafu;
use tibba_error::Error as BaseError;

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:session=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:session";

mod middleware;
mod session;

pub use middleware::*;
pub use session::*;

#[derive(Debug, Snafu)]
pub enum Error {
    /// Session ID 为空，通常表示尚未登录或 Session 已重置。
    #[snafu(display("session id is empty"))]
    SessionIdEmpty,
    /// Session ID 格式非法（长度不足 36 字符）。
    #[snafu(display("session id is invalid"))]
    SessionIdInvalid,
    /// 请求扩展中未找到 Session，通常表示 session 中间件未挂载。
    #[snafu(display("session not found"))]
    SessionNotFound,
    /// 用户未登录，HTTP 401。
    #[snafu(display("user not login"))]
    UserNotLogin,
    /// 用户无管理员权限，HTTP 403。
    #[snafu(display("user not admin"))]
    UserNotAdmin,
    /// 用户已登录但缺所需权限，HTTP 403。
    #[snafu(display("permission denied: {required}"))]
    PermissionDenied { required: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            // 服务端内部异常，返回 500
            e @ (Error::SessionIdEmpty | Error::SessionIdInvalid | Error::SessionNotFound) => {
                BaseError::new(e.to_string())
                    .with_status(500)
                    .with_exception(true)
            }

            // 未登录，返回 401
            Error::UserNotLogin => BaseError::new("user not login")
                .with_sub_category("user")
                .with_status(401)
                .with_exception(false),

            // 无管理员权限，返回 403
            Error::UserNotAdmin => BaseError::new("user not admin")
                .with_sub_category("user")
                .with_status(403)
                .with_exception(false),

            // 缺指定权限，返回 403
            Error::PermissionDenied { required } => {
                BaseError::new(format!("permission denied: {required}"))
                    .with_sub_category("permission_denied")
                    .with_status(403)
                    .with_exception(false)
            }
        };
        err.with_category("session")
    }
}

/// 权限匹配核心：判断 `granted` 集合内是否存在某一条命中 `required`。
///
/// 通配规则：
/// - `"*"` —— 命中任何 `required`（超级管理员）
/// - `"resource:*"` —— 命中所有以 `"resource:"` 为前缀的 `required`（如 `"user:*"` 命中 `"user:read"`，
///   但不命中 `"user"` 自身——必须有冒号后缀）
/// - 其余必须字符串精确相等
///
/// 空 `required` 视为非法输入，直接返回 false 而不是任意匹配，避免误授权。
pub fn permission_grants(granted: &[String], required: &str) -> bool {
    if required.is_empty() {
        return false;
    }
    granted.iter().any(|g| one_grants(g, required))
}

fn one_grants(granted: &str, required: &str) -> bool {
    if granted == "*" {
        return true;
    }
    if granted == required {
        return true;
    }
    // "resource:*" 通配前缀：required 必须以 "resource:" 起头才命中
    if let Some(prefix) = granted.strip_suffix(":*") {
        let needle = format!("{prefix}:");
        return required.starts_with(&needle) && required.len() > needle.len();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::permission_grants;

    fn g(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_required_never_matches() {
        assert!(!permission_grants(&g(&["*"]), ""));
        assert!(!permission_grants(&g(&[]), ""));
    }

    #[test]
    fn empty_granted_never_matches() {
        assert!(!permission_grants(&[], "user:read"));
    }

    #[test]
    fn exact_match() {
        assert!(permission_grants(&g(&["user:read"]), "user:read"));
        assert!(!permission_grants(&g(&["user:write"]), "user:read"));
    }

    #[test]
    fn star_grants_anything() {
        assert!(permission_grants(&g(&["*"]), "user:read"));
        assert!(permission_grants(&g(&["*"]), "anything-goes"));
    }

    #[test]
    fn resource_prefix_grants_actions() {
        assert!(permission_grants(&g(&["user:*"]), "user:read"));
        assert!(permission_grants(&g(&["user:*"]), "user:write"));
    }

    #[test]
    fn resource_prefix_does_not_match_bare_resource() {
        // "user:*" 不应命中 "user" 自身——必须有冒号后才匹配
        assert!(!permission_grants(&g(&["user:*"]), "user"));
        // 也不应命中其它 resource
        assert!(!permission_grants(&g(&["user:*"]), "file:read"));
    }

    #[test]
    fn multi_granted_takes_any_match() {
        let granted = g(&["file:read", "user:*", "system:health"]);
        assert!(permission_grants(&granted, "user:write"));
        assert!(permission_grants(&granted, "file:read"));
        assert!(!permission_grants(&granted, "file:delete"));
    }

    #[test]
    fn colon_in_required_only_matches_with_colon_prefix() {
        // granted "useradmin:*" 不应命中 "user:read"（避免子串误命中）
        assert!(!permission_grants(&g(&["useradmin:*"]), "user:read"));
    }
}
