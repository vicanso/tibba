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

//! 全局 OpenAPI 文档组装与轻量 API 文档视图挂载（零 `utoipa-swagger-ui` 重静态资源）。
//!
//! 仅在 development / test 环境挂载：
//! - `GET /api-docs/openapi.json` — OpenAPI JSON
//! - `GET /swagger-ui` — 内联 Scalar API Reference 页面

use axum::routing::get;
use axum::{Json, Router, response::Html};
use std::sync::Arc;
use tibba_util::{is_development, is_test};
use utoipa::OpenApi;
use utoipa::openapi::server::ServerBuilder;

const SCALAR_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>tibba API Reference</title>
  </head>
  <body>
    <script id="api-reference" data-url="/api-docs/openapi.json"></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </body>
</html>
"#;

#[derive(OpenApi)]
#[openapi(info(
    title = "tibba API",
    description = "tibba web 应用 HTTP API 文档（由 utoipa 在编译期自动生成）"
))]
struct ApiDoc;

/// 组装全局 OpenAPI 文档：根元数据 + `tibba_router` 文档片段。
pub fn build_openapi(api_prefix: Option<&str>) -> utoipa::openapi::OpenApi {
    let mut doc = ApiDoc::openapi();

    let base = api_prefix.unwrap_or_default();
    let url = if base.is_empty() {
        "/".to_string()
    } else {
        base.to_string()
    };
    doc.servers = Some(vec![ServerBuilder::new().url(url).build()]);
    doc.merge(tibba_router::openapi());
    doc
}

/// 仅在 development / test 环境把 `/swagger-ui` 与 `/api-docs/openapi.json` 挂到 `router`。
pub fn mount_swagger(router: Router, api_prefix: Option<&str>) -> Router {
    if !(is_development() || is_test()) {
        return router;
    }
    let doc = Arc::new(build_openapi(api_prefix));
    let docs_router = Router::new()
        .route(
            "/api-docs/openapi.json",
            get(move || {
                let doc = doc.clone();
                async move { Json((*doc).clone()) }
            }),
        )
        .route("/swagger-ui", get(|| async { Html(SCALAR_HTML) }));
    router.merge(docs_router)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_merges_common_paths() {
        let doc = build_openapi(Some("/api"));
        let paths = &doc.paths.paths;
        assert!(paths.contains_key("/healthz"));
        assert!(paths.contains_key("/readyz"));
        assert!(paths.contains_key("/commons/application"));
        assert!(paths.contains_key("/models/schema"));
        assert!(paths.contains_key("/models/create"));
        assert!(paths.contains_key("/users/login"));

        #[cfg(feature = "file")]
        {
            assert!(paths.contains_key("/files/upload"));
            assert!(paths.contains_key("/files/preview"));
        }
        #[cfg(feature = "job")]
        {
            assert!(paths.contains_key("/jobs/stats"));
        }
        #[cfg(feature = "email")]
        {
            assert!(paths.contains_key("/users/email/verify/request"));
            assert!(paths.contains_key("/users/password/reset/request"));
        }
        #[cfg(feature = "totp")]
        {
            assert!(paths.contains_key("/users/login/mfa"));
            assert!(paths.contains_key("/users/totp/enroll"));
        }
    }
}
