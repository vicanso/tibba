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

use serde_json::Value;

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
    let message = if let Ok(value) = serde_json::from_slice::<Value>(data) {
        if let Some(value) = value.get(key) {
            // Convert value to string, handling both string and non-string values
            value.as_str().map_or(value.to_string(), |s| s.to_string())
        } else {
            // Key not found
            "".to_string()
        }
    } else {
        // Invalid JSON
        "".to_string()
    };

    // Convert "null" (case-insensitive) to empty string
    if message.to_lowercase() == "null" {
        return "".to_string();
    }
    message
}
