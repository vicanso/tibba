use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use hex::encode;
use rust_embed::{EmbeddedFile, RustEmbed};

/// 项目相关静态资源数据
#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;
pub struct StaticFile(Option<EmbeddedFile>);

impl IntoResponse for StaticFile {
    fn into_response(self) -> Response {
        if self.0.is_none() {
            return StatusCode::NOT_FOUND.into_response();
        }
        // 已保证file不会为空
        let file = self.0.unwrap();
        // hash为基于内容生成
        let str = &encode(file.metadata.sha256_hash())[0..8];
        let mime_type = file.metadata.mimetype();
        // 长度+hash的一部分
        let entity_tag = format!(r#""{:x}-{str}""#, file.data.len());
        // 因为html对于网页是入口，避免缓存后更新不及时
        // 因此设置为0
        // 其它js,css会添加版本号，因此无影响
        let max_age = if mime_type.contains("text/html") {
            0
        } else {
            365 * 24 * 3600
        };

        // 缓存服务器的有效期设置为较短的值
        let server_max_age = 3600;
        let s_max_age = if max_age > server_max_age {
            Some(server_max_age)
        } else {
            None
        };

        let cache_control = if max_age <= 0 {
            "no-cache".to_string()
        } else if let Some(s_max_age) = s_max_age {
            format!("public, max-age={max_age}, s-maxage={s_max_age}")
        } else {
            format!("public, max-age={max_age}")
        };
        // 静态文件压缩由前置缓存服务器处理
        (
            [
                // content type
                (header::CONTENT_TYPE, mime_type.to_string()),
                // 为啥不设置Last-Modified
                // https://developer.mozilla.org/en-US/docs/Web/HTTP/Caching#heuristic_caching
                // e tag
                (header::ETAG, entity_tag),
                // max age
                (header::CACHE_CONTROL, cache_control),
            ],
            file.data,
        )
            .into_response()
    }
}
// 获取资源文件
fn get_asset(file_path: &str) -> Option<EmbeddedFile> {
    Assets::get(file_path)
}
// 获取静态资源文件
pub fn get_static_file(file_path: &str) -> StaticFile {
    let file = get_asset(file_path);
    StaticFile(file)
}

// 获取资源文件并返回字符串(trim后)
pub fn get_string_from_asset(file_path: &str) -> String {
    if let Some(file) = get_asset(file_path) {
        std::string::String::from_utf8_lossy(&file.data)
            .trim()
            .to_string()
    } else {
        "".to_string()
    }
}

// 获取程序构建日期
pub fn get_build_date() -> String {
    get_string_from_asset("build_date")
}

// 获取git的commit id
pub fn get_commit() -> String {
    get_string_from_asset("commit")
}
