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

//! 不依赖 Postgres/Redis 的契约级测试：中间件配置面与错误 JSON 形状。

use tibba_error::Error as TibbaError;
use tibba_middleware::{Cors, MiddlewareOptions};

#[test]
fn middleware_options_default_and_minimal() {
    let full = MiddlewareOptions::default();
    assert!(full.csrf && full.api_key && full.http_cache);

    let min = MiddlewareOptions::minimal().with_csrf(true);
    assert!(min.csrf);
    assert!(!min.api_key);
    assert!(!min.otel);
}

#[test]
fn cors_production_safe_requires_origins() {
    let open = Cors::default();
    assert!(open.is_open());
    assert!(open.assert_production_safe().is_err());

    let locked = Cors::new().add_allow_origin("https://app.example.com");
    assert!(!locked.is_open());
    assert!(locked.assert_production_safe().is_ok());
    assert_eq!(
        locked.allow_origins(),
        &["https://app.example.com".to_string()]
    );
}

#[test]
fn tibba_error_json_is_flat() {
    let err = TibbaError::new("rate limited")
        .with_category("middleware")
        .with_sub_category("rate_limited")
        .with_status(429);
    let v = serde_json::to_value(&err).expect("serialize");
    assert_eq!(v["category"], "middleware");
    assert_eq!(v["message"], "rate limited");
    assert_eq!(v["sub_category"], "rate_limited");
    // status 不在序列化视图里（仅 HTTP IntoResponse 使用）；确保无嵌套 data 字段
    assert!(v.get("data").is_none());
}

#[test]
fn permission_grants_wildcard() {
    use tibba_session::permission_grants;
    let granted = vec!["model:user:*".to_string(), "file:read".to_string()];
    assert!(permission_grants(&granted, "model:user:read"));
    assert!(permission_grants(&granted, "model:user:write"));
    assert!(!permission_grants(&granted, "model:token:read"));
    assert!(permission_grants(&["*".to_string()], "anything"));
}

#[tokio::test]
async fn stats_and_tracker_without_entry_do_not_panic() {
    use axum::Router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use std::net::SocketAddr;
    use tibba_middleware::{stats, user_tracker};
    use tibba_runtime::AppState;
    use tower::ServiceExt;

    let state: &'static AppState = Box::leak(Box::new(AppState::new("test-commit")));
    let app = Router::new()
        .route("/ping", get(|| async { "pong" }))
        .layer(from_fn_with_state(("user", "login").into(), user_tracker))
        .layer(from_fn_with_state(state, stats));

    let mut req = Request::builder()
        .uri("/ping")
        .body(Body::empty())
        .expect("build request");
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 8080))));

    let resp = app.oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn i18n_middleware_localizes_error_and_sets_vary() {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use axum::middleware::from_fn;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use http_body_util::BodyExt;
    use tibba_i18n::{Catalog, i18n};
    use tower::ServiceExt;

    let _ = tibba_i18n::init(
        Catalog::new("en")
            .add("en", "not_logged_in", "Authentication required.")
            .add("zh", "not_logged_in", "请先登录后再操作。"),
    );

    let app = Router::new()
        .route(
            "/err",
            get(|| async {
                TibbaError::new("raw message")
                    .with_category("session")
                    .with_sub_category("not_logged_in")
                    .with_status(401)
                    .into_response()
            }),
        )
        .layer(from_fn(i18n));

    let req = Request::builder()
        .uri("/err")
        .header(header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let has_vary_lang = resp
        .headers()
        .get_all(header::VARY)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| v.to_ascii_lowercase().contains("accept-language"));
    assert!(has_vary_lang, "Vary header must include accept-language");

    let body = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(json["message"], "请先登录后再操作。");
}

#[tokio::test]
async fn csrf_middleware_enforces_cookie_requests_and_exempts_pure_bearer() {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use axum::middleware::from_fn;
    use axum::routing::post;
    use tibba_middleware::validate_csrf;
    use tower::ServiceExt;

    let app = Router::new()
        .route("/mutate", post(|| async { "ok" }))
        .layer(from_fn(validate_csrf));

    // 带 Cookie 且附带空 Bearer 头仍必须拦截（403）
    let blocked_req = Request::builder()
        .method(Method::POST)
        .uri("/mutate")
        .header(header::COOKIE, "tibba_sid=abc")
        .header(header::AUTHORIZATION, "Bearer   ")
        .body(Body::empty())
        .expect("build blocked req");
    let blocked_resp = app.clone().oneshot(blocked_req).await.expect("oneshot");
    assert_eq!(blocked_resp.status(), StatusCode::FORBIDDEN);

    // 纯 API Key / Bearer 请求（不带 Cookie）豁免 CSRF
    let api_req = Request::builder()
        .method(Method::POST)
        .uri("/mutate")
        .header(header::AUTHORIZATION, "Bearer tibba_token")
        .body(Body::empty())
        .expect("build api req");
    let api_resp = app.oneshot(api_req).await.expect("oneshot");
    assert_eq!(api_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn http_cache_downgrades_auth_responses_and_preserves_headers_on_304() {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use tibba_middleware::{HttpCache, http_cache};
    use tower::ServiceExt;

    let app = Router::new()
        .route(
            "/item",
            get(|| async {
                let mut resp = axum::response::Response::new(Body::from("hello-cached-body"));
                resp.headers_mut().insert(
                    header::CONTENT_LENGTH,
                    header::HeaderValue::from_static("17"),
                );
                resp
            }),
        )
        .layer(from_fn_with_state(HttpCache::default(), http_cache));

    let req1 = Request::builder()
        .uri("/item")
        .header(header::AUTHORIZATION, "Bearer tok")
        .body(Body::empty())
        .expect("build req1");
    let resp1 = app.clone().oneshot(req1).await.expect("oneshot1");
    assert_eq!(resp1.status(), StatusCode::OK);
    let etag = resp1
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .expect("ETag present")
        .to_string();
    assert_eq!(
        resp1
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("private, no-cache")
    );
    let vary_all: String = resp1
        .headers()
        .get_all(header::VARY)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<_>>()
        .join(",")
        .to_ascii_lowercase();
    assert!(vary_all.contains("authorization"), "vary: {vary_all}");
    assert!(vary_all.contains("cookie"), "vary: {vary_all}");

    let req2 = Request::builder()
        .uri("/item")
        .header(header::AUTHORIZATION, "Bearer tok")
        .header(header::IF_NONE_MATCH, etag.as_str())
        .body(Body::empty())
        .expect("build req2");
    let resp2 = app.oneshot(req2).await.expect("oneshot2");
    assert_eq!(resp2.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        resp2
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok()),
        Some(etag.as_str())
    );
    assert_eq!(
        resp2
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("private, no-cache")
    );
}
