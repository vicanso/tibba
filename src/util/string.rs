use bytes::Bytes;
use nanoid::nanoid;
use serde_json::Value;
use uuid::Uuid;

pub fn random_string(size: usize) -> String {
    nanoid!(size)
}

pub fn json_get(data: &Bytes, key: &str) -> String {
    let message = if let Ok(value) = serde_json::from_slice::<Value>(data) {
        value.get(key).unwrap_or(&Value::Null).to_string()
    } else {
        "".to_string()
    };
    // 处理为""
    if message.to_lowercase() == "null" {
        return "".to_string();
    }
    message
}

pub fn uuid() -> String {
    Uuid::new_v4().to_string()
}
