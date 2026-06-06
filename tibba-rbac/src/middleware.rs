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

use axum::extract::{Request, State};
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
/// Session 从请求扩展中获取，因此调用方必须先在外层挂 `tibba_session::session` 中间件。
pub async fn require_permission(
    State(required): State<&'static str>,
    req: Request,
    next: Next,
) -> Result<Response> {
    // Session 由上游 session 中间件注入请求扩展；此处只读不改
    let session = req
        .extensions()
        .get::<Session>()
        .cloned()
        .ok_or_else(|| {
            // 这一分支表示路由配置错误：require_permission 必须挂在 session 中间件之后
            BaseError::new("session middleware not mounted before require_permission")
                .with_category("rbac")
                .with_status(500)
                .with_exception(true)
        })?;

    session.require_permission(required)?;
    Ok(next.run(req).await)
}
