use nanoid::nanoid;
use serde_json::Value;

pub fn random_string(size: usize) -> String {
    nanoid!(size)
}

pub fn json_get(data: &str, key: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(data) {
        return value.get(key).unwrap_or(&Value::Null).to_string();
    }
    "".to_string()
}
