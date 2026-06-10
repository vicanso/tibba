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

//! 全局 OpenAPI 文档组装与 Swagger UI 挂载。
//!
//! ## 渐进式设计
//! 根文档 [`ApiDoc`] 只持有 info 元数据；各 `tibba-router-*` 模块各自用
//! `#[utoipa::path]` 注解端点、用 `#[derive(OpenApi)]` 聚合成片段，并导出
//! `pub fn openapi()`。本模块在 [`build_openapi`] 里把这些片段逐个 `merge`
//! 进根文档——新模块注解完成后只需在此追加一行 `merge`，无需改动其它处。
//!
//! ## 暴露策略
//! Swagger UI 仅在 development / test 环境挂载（见 [`mount_swagger`]），
//! 生产环境不暴露任何 API 表面。

use axum::Router;
use tibba_util::{is_development, is_test};
use utoipa::OpenApi;
use utoipa::openapi::server::ServerBuilder;
use utoipa_swagger_ui::SwaggerUi;

/// 全局 OpenAPI 文档根。
///
/// 仅声明 info 元数据；`version` 由 utoipa 自动取主 crate 的
/// `CARGO_PKG_VERSION`，避免文档与二进制版本漂移。具体端点与 schema
/// 通过 [`build_openapi`] 合并各路由模块片段得到。
#[derive(OpenApi)]
#[openapi(info(
    title = "tibba API",
    description = "tibba web 应用 HTTP API 文档（由 utoipa 在编译期自动生成）"
))]
struct ApiDoc;

/// 组装全局 OpenAPI 文档：根元数据 + 各路由模块片段。
///
/// `api_prefix` 为部署侧配置的 API 前缀（如 `/api`），写进 `servers`，
/// 使 Swagger UI 的「Try it out」请求带上正确前缀；为空时退化为 `/`。
fn build_openapi(api_prefix: Option<&str>) -> utoipa::openapi::OpenApi {
    let mut doc = ApiDoc::openapi();

    // servers 指向 API 前缀；注解里的路径都是前缀内的相对路径
    let base = api_prefix.unwrap_or_default();
    let url = if base.is_empty() {
        "/".to_string()
    } else {
        base.to_string()
    };
    doc.servers = Some(vec![ServerBuilder::new().url(url).build()]);

    // 合并各路由模块文档片段。各 crate 的 #[utoipa::path] 注解里已写明含 nest
    // 前缀的绝对路径（/files、/models、/users），故此处直接 merge 无需再加前缀。
    doc.merge(tibba_router_common::openapi());
    doc.merge(tibba_router_file::openapi());
    doc.merge(tibba_router_model::openapi());
    doc.merge(tibba_router_user::openapi());

    doc
}

/// 仅在 development / test 环境，把 Swagger UI 与 openapi.json 挂到 `router`。
///
/// 生产环境原样返回 `router`，不暴露任何文档端点。挂载路径：
/// - `GET /swagger-ui` — Swagger UI 页面
/// - `GET /api-docs/openapi.json` — OpenAPI JSON
///
/// 返回值已是 `#[must_use]` 的 `Router`，无需额外标注。
pub fn mount_swagger(router: Router, api_prefix: Option<&str>) -> Router {
    if !(is_development() || is_test()) {
        return router;
    }
    let doc = build_openapi(api_prefix);
    router.merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证根文档正确合并了 common 路由片段，且 servers 反映 API 前缀。
    #[test]
    fn openapi_merges_common_paths() {
        let doc = build_openapi(Some("/api"));

        // common 路由注解的 4 个端点应全部出现在合并后的文档里
        let paths = &doc.paths.paths;
        assert!(paths.contains_key("/healthz"));
        assert!(paths.contains_key("/readyz"));
        assert!(paths.contains_key("/commons/application"));
        assert!(paths.contains_key("/commons/captcha"));

        // file / model / user 三路由片段应按各自 nest 前缀合并进来
        assert!(paths.contains_key("/files/upload"));
        assert!(paths.contains_key("/files/preview"));
        assert!(paths.contains_key("/models/schema"));
        assert!(paths.contains_key("/models/create"));
        assert!(paths.contains_key("/users/login"));
        assert!(paths.contains_key("/users/login/mfa"));
        assert!(paths.contains_key("/users/totp/enroll"));
        assert!(paths.contains_key("/users/oauth/github/start"));

        // servers 应反映传入的 API 前缀
        let servers = doc.servers.as_ref().expect("servers should be set");
        assert_eq!(servers[0].url, "/api");
    }

    /// 空前缀时 servers 退化为 "/"。
    #[test]
    fn openapi_default_server_is_root() {
        let doc = build_openapi(None);
        let servers = doc.servers.as_ref().expect("servers should be set");
        assert_eq!(servers[0].url, "/");
    }
}
