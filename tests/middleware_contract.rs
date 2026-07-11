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
