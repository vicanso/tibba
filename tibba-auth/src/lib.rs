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

//! tibba-auth
//!
//! 统一认证与授权 crate：默认提供 Session + Cookie（含 Redis 存储）与 RBAC 路由中间件，
//! 并通过 feature 按需开启可选认证能力：
//!
//! - `jwt`：HS256 Access Token + Redis 不透明 Refresh Token 认证路径
//! - `totp`：RFC 6238 TOTP 两步验证核心与密钥落库加密
//! - `oauth`：第三方 OAuth2 提供商客户端（GitHub、Google）

#[cfg(feature = "jwt")]
pub mod jwt;
#[cfg(feature = "oauth")]
pub mod oauth;
pub mod rbac;
pub mod session;
#[cfg(feature = "totp")]
pub mod totp;

pub use rbac::require_permission;
pub use session::{
    AdminSession, Error, Role, Session, SessionParams, SessionResponse, UserSession,
    permission_grants, revoke_role_sessions, revoke_user_sessions, session as session_middleware,
};
