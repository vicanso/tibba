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

use axum::extract::{FromRequestParts, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use tibba_error::Error as BaseError;
use tibba_session::Session;

type Result<T, E = BaseError> = std::result::Result<T, E>;

/// axum 中间件：要求当前 Session 已登录且持有 `required` 权限码。
///
/// 用 `from_fn_with_state(&'static str, require_permission)` 挂在路由上：
///
/// ```ignore
/// .route("/users/:id", delete(delete_user))
///     .layer(from_fn_with_state("user:delete", require_permission))
/// ```
///
/// 鉴权失败时直接返回 `tibba_error::Error`（未登录 → 401，缺权限 → 403），
/// 由全局错误处理统一渲染响应。
///
/// Session 经 `Session` 提取器加载，因此调用方必须先在外层挂 `tibba_session::session` 中间件。
pub async fn require_permission(
    State(required): State<&'static str>,
    req: Request,
    next: Next,
) -> Result<Response> {
    let (mut parts, body) = req.into_parts();
    // 必须用 Session 提取器：它会从签名 cookie / Redis 实际加载会话数据。直接读 extensions
    // 里的 Session 只会拿到 session 中间件放入的空壳（iat==0），导致已登录用户也恒被拒（fail-closed）。
    let session = Session::from_request_parts(&mut parts, &()).await?;
    session.require_permission(required)?;
    Ok(next.run(Request::from_parts(parts, body)).await)
}
