// Copyright 2025 Tree xie.
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

use serde_json::{Map, Value};

fn json_value_to_string(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        // if the value is a string, return the content
        s.to_string()
    } else if v.is_null() {
        // if the value is null, return an empty string
        String::new()
    } else {
        // for other types (number, boolean, etc.), use to_string() to convert
        v.to_string()
    }
}

/// Extracts multiple string values from JSON data by keys
///
/// This function provides a safe way to extract multiple string values from JSON with the following features:
/// - Handles invalid JSON gracefully
/// - Converts non-string values to strings
/// - Returns empty strings for missing keys
/// - Maintains order of results matching input keys
///
/// # Arguments
/// * `data` - Byte slice containing JSON data
/// * `keys` - Array of keys to look up in the JSON object
///
/// # Returns
/// * Vector of strings containing the values, with empty strings for:
///   - JSON is invalid
///   - Key doesn't exist
///   - Value cannot be converted to string
///
/// # Examples
/// ```
/// let json = r#"{"name": "John", "age": 30}"#.as_bytes();
/// let results = json_get_strings(json, &["name", "age", "missing"]);
/// assert_eq!(vec!["John", "30", ""], results);
/// ```
pub fn json_get_strings(data: &[u8], keys: &[&str]) -> Vec<String> {
    // try to parse JSON, return an empty string Vec if parsing fails
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return vec![String::new(); keys.len()];
    };

    keys.iter()
        .map(|key| get_map_string(value.as_object().unwrap(), key))
        .collect()
}

/// Extracts a string value from JSON data by key
///
/// This function provides a safe way to extract string values from JSON with the following features:
/// - Handles invalid JSON gracefully
/// - Converts non-string values to strings
/// - Treats "null" values as empty strings
/// - Returns empty string for missing keys
///
/// # Arguments
/// * `data` - Byte slice containing JSON data
/// * `key` - Key to look up in the JSON object
///
/// # Returns
/// * String containing the value or empty string if:
///   - JSON is invalid
///   - Key doesn't exist
///   - Value is null
///   - Value cannot be converted to string
///
/// # Examples
/// ```
/// let json = r#"{"name": "John", "age": 30, "null_value": null}"#.as_bytes();
/// assert_eq!("John", json_get(json, "name"));
/// assert_eq!("30", json_get(json, "age"));
/// assert_eq!("", json_get(json, "null_value"));
/// assert_eq!("", json_get(json, "non_existent"));
/// ```
pub fn json_get(data: &[u8], key: &str) -> String {
    serde_json::from_slice::<Value>(data)
        .ok()
        .as_ref()
        .and_then(|v| v.get(key))
        .map_or(String::new(), json_value_to_string)
}

/// Extracts a string value from a JSON map by key
///
/// This function provides a safe way to extract string values from a JSON map with the following features:
/// - Handles missing keys gracefully
/// - Converts non-string values to strings
/// - Returns empty string for missing keys
///
/// # Arguments
/// * `data` - JSON map to extract value from
/// * `key` - Key to look up in the JSON map
///
/// # Returns
/// * String containing the value or empty string if:
///   - Key doesn't exist
///   - Value cannot be converted to string
pub fn get_map_string(data: &Map<String, Value>, key: &str) -> String {
    data.get(key).map_or(String::new(), json_value_to_string)
}
