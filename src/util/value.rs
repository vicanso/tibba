use crate::error::HttpError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde_json::Value;

pub type Result<T, E = HttpError> = std::result::Result<T, E>;

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

pub fn json_get_i64(value: &Value, key: &str) -> Result<Option<i64>> {
    if let Some(value) = value.get(key) {
        if !value.is_i64() {
            return Err(HttpError::new(&format!("{key} is not a number")));
        }
        return Ok(value.as_i64());
    }
    Ok(None)
}

pub fn json_get_string(value: &Value, key: &str) -> Result<Option<String>> {
    if let Some(value) = value.get(key) {
        if !value.is_string() {
            return Err(HttpError::new(&format!("{key} is not a string")));
        }
        if let Some(value) = value.as_str() {
            return Ok(Some(value.to_string()));
        }
    }
    Ok(None)
}

pub fn json_get_date_time(value: &Value, key: &str) -> Result<Option<DateTime<Utc>>> {
    if let Some(value) = json_get_string(value, key)? {
        let value = value
            .parse::<DateTime<Utc>>()
            .map_err(|err| HttpError::new(&err.to_string()))?;
        return Ok(Some(value));
    }
    Ok(None)
}

pub fn json_value_to_strings(value: &Value) -> Result<Option<Vec<String>>> {
    if !value.is_array() {
        return Err(HttpError::new("value is not an array"));
    }
    if let Some(values) = value.as_array() {
        let mut err = None;
        let arr = values
            .iter()
            .map(|item| {
                if !item.is_string() {
                    err = Some(HttpError::new("value is not a string"));
                    return "".to_string();
                }
                return item.as_str().unwrap_or_default().to_string();
            })
            .collect();
        // 如果出错
        if let Some(err) = err {
            return Err(err);
        }
        return Ok(Some(arr));
    }
    Ok(None)
}

pub fn json_get_strings(value: &Value, key: &str) -> Result<Option<Vec<String>>> {
    if let Some(arr) = value.get(key) {
        return json_value_to_strings(arr);
    }
    Ok(None)
}

pub fn json_get_bytes(value: &Value, key: &str) -> Result<Option<Vec<u8>>> {
    if let Some(value) = json_get_string(value, key)? {
        let value = STANDARD
            .decode(value)
            .map_err(|err| HttpError::new_with_category(&err.to_string(), "base64"))?;
        return Ok(Some(value));
    }
    Ok(None)
}
