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

//! tibba-rbac
//!
//! 路由层 RBAC 中间件。匹配逻辑 (`permission_grants`) 与 `Error::PermissionDenied`
//! 都定义在 `tibba-session` 里，本 crate 仅提供 axum middleware 适配，把
//! `require_permission(...)` 作为 layer 挂到具体路由前。
//!
//! ## 用法
//!
//! ```ignore
//! use axum::middleware::from_fn_with_state;
//! use axum::routing::delete;
//! use tibba_rbac::require_permission;
//!
//! Router::new()
//!     .route("/users/:id", delete(delete_user))
//!     .layer(from_fn_with_state("user:delete", require_permission))
//! ```
//!
//! handler 内部如需更灵活的判断，可直接调用 `Session::has_permission` /
//! `Session::require_permission`，不需要本 crate。
//!
//! ## 管理角色权限
//!
//! 收回权限请走 [`RbacAdmin`]：它在改完 `role_permissions` 之后让受影响角色的
//! 已登录凭证失效。直接调 `RolePermissionModel::revoke` 只改数据库，会话 / JWT 中
//! 缓存的权限并集要到过期才会收窄。

pub use admin::RbacAdmin;
pub use middleware::require_permission;

mod admin;
mod middleware;
