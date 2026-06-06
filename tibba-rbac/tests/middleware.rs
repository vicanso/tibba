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

//! tibba-rbac 中间件集成测试。
//!
//! 这里只覆盖中间件的「装配/路由配置」错误路径——session 中间件未挂载时返回 500。
//! 「已挂载 session 但缺权限 → 403」「拥有权限 → 200」两条路径的核心逻辑由
//! `tibba_session::permission_grants` 的单元测试（tibba-session/src/lib.rs）覆盖，
//! 在不引入 Redis fixture 的前提下重复造 axum 全链路收益有限。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::{Router, response::IntoResponse};
use tibba_rbac::require_permission;
use tower::ServiceExt;

async fn ok_handler() -> impl IntoResponse {
    "ok"
}

#[tokio::test]
async fn returns_500_when_session_middleware_not_mounted() {
    // 故意不挂 session 中间件，模拟路由配置错误：require_permission 无法在请求扩展中找到 Session
    let app = Router::new()
        .route("/protected", get(ok_handler))
        .layer(from_fn_with_state("user:read", require_permission));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // require_permission 返回 BaseError::with_status(500)
    // axum 把它经全局错误处理映射为 500 状态码
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
