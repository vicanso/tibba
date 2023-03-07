use axum::response::{IntoResponse, Response};
use hex::encode;
use http::{header, StatusCode};
/// 项目相关静态资源数据
use rust_embed::{EmbeddedFile, RustEmbed};

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

pub struct StaticFile {
    file_path: String,
    hash: String,
    max_age: u32,
    file: Option<EmbeddedFile>,
}

impl IntoResponse for StaticFile {
    fn into_response(self) -> Response {
        if let Some(file) = self.file {
            let guess = mime_guess::from_path(&self.file_path);
            (
                [
                    // content type
                    (header::CONTENT_TYPE, guess.first_or_octet_stream().as_ref()),
                    // 为啥不设置Last-Modified
                    // https://developer.mozilla.org/en-US/docs/Web/HTTP/Caching#heuristic_caching
                    // e tag
                    (header::ETAG, self.hash.as_str()),
                    // max age
                    (
                        header::CACHE_CONTROL,
                        format!("public, max-age={}", self.max_age).as_str(),
                    ),
                ],
                file.data,
            )
                .into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

// 获取资源文件
fn get_asset(file_path: &str) -> Option<EmbeddedFile> {
    Assets::get(file_path)
}

// 获取静态资源文件
pub fn get_static_file(file_path: &str) -> StaticFile {
    let mut hash = "".to_string();
    let file = get_asset(file_path);
    if let Some(ref value) = file {
        let str = &encode(value.metadata.sha256_hash())[0..8];
        // 长度+hash一部分
        hash = format!("{:x}-{str}", value.data.len())
    }
    // 因为html对于网页是入口，避免缓存后更新不及时
    // 因此设置为0
    // 其它js,css会添加版本号，因此无影响
    let mut max_age = 3600;
    if file_path.ends_with(".html") {
        max_age = 0
    }
    StaticFile {
        max_age,
        file_path: file_path.to_string(),
        hash,
        file,
    }
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
