use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

fn to_string(v: &[u8]) -> String {
    std::string::String::from_utf8_lossy(v).to_string()
}

pub fn get_build_date() -> String {
    if let Some(file) = Assets::get("build_date") {
        return to_string(&file.data).trim().to_string();
    }
    "".to_string()
}
