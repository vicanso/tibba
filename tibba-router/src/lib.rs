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

pub mod common;
#[cfg(feature = "file")]
pub mod file;
#[cfg(feature = "job")]
pub mod job;
pub mod model;
pub mod rbac;
pub mod user;
pub mod user_admin;

/// 合并所有已启用路由模块的 OpenAPI 文档片段，供主应用挂载或导出。
pub fn openapi() -> utoipa::openapi::OpenApi {
    let mut doc = common::openapi();
    doc.merge(user::openapi());
    doc.merge(model::openapi());
    #[cfg(feature = "file")]
    doc.merge(file::openapi());
    #[cfg(feature = "job")]
    doc.merge(job::openapi());
    doc
}
