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

use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};

#[cfg(feature = "embed-admin")]
#[derive(rust_embed::RustEmbed)]
#[folder = "admin/dist/"]
struct WebAssets;

#[cfg(feature = "embed-admin")]
fn file_response(path: &str, data: std::borrow::Cow<'static, [u8]>) -> Response {
    use axum::body::Bytes;
    use axum::http::header;

    let mime = mime_guess2::from_path(path).first_or_octet_stream();
    // assets/ 子目录的文件名含 vite 内容哈希，可长期缓存
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, mime.as_ref()),
            (header::CACHE_CONTROL, cache_control),
        ],
        Bytes::copy_from_slice(&data),
    )
        .into_response()
}

/// 静态文件服务：启用 `embed-admin` 时服务内嵌 `admin/dist/` 资源并做 SPA fallback；
/// 未启用时直接返回 404（供纯 API 或前端独立部署场景）。
pub(crate) async fn serve_web(prefix: &str, uri: Uri) -> Response {
    #[cfg(feature = "embed-admin")]
    {
        let raw = uri.path();
        let path = if prefix.is_empty() {
            raw
        } else {
            raw.strip_prefix(prefix).unwrap_or(raw)
        };
        let path = path.trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };

        if let Some(content) = WebAssets::get(path) {
            return file_response(path, content.data);
        }

        // SPA 回退：未匹配的路径交给前端路由处理
        match WebAssets::get("index.html") {
            Some(content) => file_response("index.html", content.data),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }
    #[cfg(not(feature = "embed-admin"))]
    {
        let _ = (prefix, uri);
        StatusCode::NOT_FOUND.into_response()
    }
}
