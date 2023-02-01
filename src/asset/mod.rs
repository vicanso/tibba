/// 项目相关静态资源数据
use rust_embed::{EmbeddedFile, RustEmbed};

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

// 获取资源文件
pub fn get_asset(file_path: &str) -> Option<EmbeddedFile> {
    Assets::get(file_path)
}

// 获取资源文件并返回字符串(trim后)
pub fn get_string_from_asset(file_path: &str) -> String {
    if let Some(file) = get_asset(file_path) {
        return std::string::String::from_utf8_lossy(&file.data)
            .trim()
            .to_string();
    }
    "".to_string()
}

// 获取程序构建日期
pub fn get_build_date() -> String {
    get_string_from_asset("build_date")
}

// 获取git的commit id
pub fn get_commit() -> String {
    get_string_from_asset("commit")
}
