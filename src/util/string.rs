use nanoid::nanoid;
use serde_json::Value;

pub fn random_string(size: usize) -> String {
    nanoid!(size)
}

pub fn json_get(data: &str, key: &str) -> String {
    let message = if let Ok(value) = serde_json::from_str::<Value>(data) {
        value.get(key).unwrap_or(&Value::Null).to_string()
    } else {
        "".to_string()
    };
    // 处理为""
    if message == "null" {
        return "".to_string();
    }
    message
}
