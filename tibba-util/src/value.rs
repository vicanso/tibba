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
    // Initialize result vector with empty strings matching keys length
    let mut result = vec!["".to_string(); keys.len()];

    // Try to parse JSON, return empty strings if parsing fails
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return result;
    };

    // Process each key and populate result vector
    for (i, key) in keys.iter().enumerate() {
        let value = if let Some(value) = value.get(key) {
            // Convert value to string, handling both string and non-string values
            value.as_str().map_or(value.to_string(), |s| s.to_string())
        } else {
            // Key not found, use empty string
            "".to_string()
        };
        result[i] = value;
    }

    result
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
    json_get_strings(data, &[key])[0].clone()
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
    if let Some(value) = data.get(key) {
        value.as_str().map_or(value.to_string(), |s| s.to_string())
    } else {
        "".to_string()
    }
}
