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

//! 将合并后的 OpenAPI 文档写出到文件，供 admin 生成 TS client 或契约 diff。
//!
//! ```bash
//! cargo run --bin export-openapi -- admin/openapi.json
//! # 然后（可选，需本机 openapi-typescript）：
//! # npx openapi-typescript admin/openapi.json -o admin/src/api/schema.d.ts
//! ```

use utoipa::OpenApi;
use utoipa::openapi::server::ServerBuilder;

#[derive(OpenApi)]
#[openapi(info(
    title = "tibba API",
    description = "tibba web 应用 HTTP API（export-openapi 离线导出）"
))]
struct ApiDoc;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "openapi.json".to_string());
    let prefix = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "/api".to_string());

    let mut doc = ApiDoc::openapi();
    doc.servers = Some(vec![ServerBuilder::new().url(prefix).build()]);
    // 与 src/openapi.rs::build_openapi 保持同一 merge 列表
    doc.merge(tibba_router_common::openapi());
    doc.merge(tibba_router_file::openapi());
    doc.merge(tibba_router_model::openapi());
    doc.merge(tibba_router_user::openapi());

    let json = serde_json::to_string_pretty(&doc).expect("serialize openapi");
    std::fs::write(&path, json).unwrap_or_else(|e| panic!("write {path}: {e}"));
    eprintln!("wrote OpenAPI to {path}");
}
