use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde_json::Value;

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

pub fn json_get_i64(value: &Value, key: &str) -> Option<i64> {
    if let Some(value) = value.get(key) {
        return value.as_i64();
    }
    None
}
pub fn json_get_string(value: &Value, key: &str) -> Option<String> {
    if let Some(value) = value.get(key) {
        if let Some(value) = value.as_str() {
            return Some(value.to_string());
        }
    }
    None
}

pub fn json_get_date_time(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    if let Some(value) = json_get_string(value, key) {
        // println!("{value:?}");
        // println!("{:?}",  value.parse::<DateTime<Utc>>());
        if let Ok(value) = value.parse::<DateTime<Utc>>() {
            return Some(value);
        }
    }
    None
}
