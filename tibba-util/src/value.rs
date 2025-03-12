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

pub fn json_get(data: &[u8], key: &str) -> String {
    let message = if let Ok(value) = serde_json::from_slice::<Value>(data) {
        if let Some(value) = value.get(key) {
            value.as_str().map_or(value.to_string(), |s| s.to_string())
        } else {
            "".to_string()
        }
    } else {
        "".to_string()
    };
    // if message is null, return ""
    if message.to_lowercase() == "null" {
        return "".to_string();
    }
    message
}
