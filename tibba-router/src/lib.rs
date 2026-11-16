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

//! Tibba HTTP 路由与后台管理入口聚合层。
//!
//! - [`common`]：`/healthz`、`/readyz` 与 `/commons/application`
//! - [`user`]：登录 / 注册 / 会话 / 邮箱验证 / 密码重置，以及按 feature 启用
//!   的 `jwt` / `totp` / `oauth` / `api-key`
//! - [`model`]：schema 驱动的通用模型 CRUD 路由与动态注册表
//! - [`user_admin`]：`user` 模型的安全包装（禁自残/防越权/改身份撤销会话）
//! - [`rbac`]：`RbacAdmin` 角色权限管理与撤销辅助
//! - [`file`]（`feature = "file"`）：multipart 上传、预览、Range 下载与预签名
//! - [`job`]（`feature = "job"`）：任务队列统计与死信队列运维端点

//! - [`sql_query`]（`feature = "sql-query"`）：运行期配置化只读 SQL 查询接口（`/queries/{group}/{name}` 与 `/sql-queries`）
//! - [`app_auth`]（`feature = "app-auth"`）：应用级签名鉴权中间件（`app_id + ts + sign`）与 `/apps` SQL 查询接口授权管理

#[cfg(feature = "app-auth")]
pub mod app_auth;
pub mod audit;
pub mod common;
pub mod configuration;
#[cfg(feature = "file")]
pub mod file;
#[cfg(feature = "job")]
pub mod job;
pub mod model;
pub mod rbac;
#[cfg(feature = "sql-query")]
pub mod sql_query;
pub mod user;
pub mod user_admin;

/// 已通过应用签名（`app_id + ts + sign`）认证的内部服务身份扩展。
///
/// 仅用于授权调用 `allowed_queries` 匹配的只读 SQL 查询接口（支持精确匹配 `{group}/{name}`
/// 或分组通配 `{group}/*`），不授予通用登录 [`tibba_auth::Session`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppClientIdentity {
    pub app_id: String,
    pub allowed_queries: Vec<String>,
}

impl AppClientIdentity {
    /// 判断当前应用是否被授权调用 `{group}/{name}` 查询接口。
    #[must_use]
    pub fn allows_query(&self, group: &str, name: &str) -> bool {
        let exact = format!("{group}/{name}");
        let group_wildcard = format!("{group}/*");
        self.allowed_queries
            .iter()
            .any(|q| q == &exact || q == &group_wildcard)
    }
}

/// 从请求头提取截断前的 User-Agent 字符串，缺失或非 UTF-8 时返回空串。
pub(crate) fn user_agent_of(headers: &axum::http::HeaderMap) -> String {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// 合并所有已启用路由模块的 OpenAPI 文档片段，供主应用挂载或导出。
pub fn openapi() -> utoipa::openapi::OpenApi {
    let mut doc = common::openapi();
    doc.merge(user::openapi());
    doc.merge(model::openapi());
    doc.merge(rbac::openapi());
    doc.merge(audit::openapi());
    doc.merge(configuration::openapi());
    #[cfg(feature = "file")]
    doc.merge(file::openapi());
    #[cfg(feature = "job")]
    doc.merge(job::openapi());
    #[cfg(feature = "sql-query")]
    doc.merge(sql_query::openapi());
    #[cfg(feature = "app-auth")]
    doc.merge(app_auth::openapi());
    doc
}

#[cfg(test)]
mod tests {
    use super::AppClientIdentity;

    #[test]
    fn app_client_identity_matches_exact_and_group_wildcard() {
        let identity = AppClientIdentity {
            app_id: "billing_svc".to_string(),
            allowed_queries: vec!["report/*".to_string(), "user/recent_orders".to_string()],
        };
        assert!(identity.allows_query("report", "orders"));
        assert!(identity.allows_query("report", "any_future_query"));
        assert!(identity.allows_query("user", "recent_orders"));
        assert!(!identity.allows_query("user", "other_query"));
        assert!(!identity.allows_query("admin", "secret"));
    }
}
