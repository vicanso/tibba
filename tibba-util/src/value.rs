// Copyright 2026 Tree xie.
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

use serde::Deserialize;
use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::fmt;

fn json_value_to_string(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if v.is_null() {
        String::new()
    } else {
        v.to_string()
    }
}

/// 仅对命中的目标字段值做轻量反序列化并转为字符串；其余字段通过 `IgnoredAny` 跳过。
struct StringifiedValue(String);

impl<'de> Deserialize<'de> for StringifiedValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;

        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = StringifiedValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any json value")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StringifiedValue(
                    if v { "true" } else { "false" }.to_string(),
                ))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StringifiedValue(v.to_string()))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StringifiedValue(v.to_string()))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
                // 使用 serde_json 与 Value::to_string 对齐浮点格式（如 1.0 -> "1.0"）
                Ok(StringifiedValue(
                    serde_json::to_string(&v).unwrap_or_else(|_| v.to_string()),
                ))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StringifiedValue(v.to_string()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
                Ok(StringifiedValue(v))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(StringifiedValue(String::new()))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(StringifiedValue(String::new()))
            }

            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
                StringifiedValue::deserialize(d)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
                let v = Value::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))?;
                Ok(StringifiedValue(v.to_string()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let v = Value::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(StringifiedValue(v.to_string()))
            }
        }

        deserializer.deserialize_any(ValueVisitor)
    }
}

struct TopLevelObjectVisitor<'a> {
    keys: &'a [&'a str],
    out: &'a mut [String],
}

impl<'de> Visitor<'de> for TopLevelObjectVisitor<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any json value")
    }

    fn visit_bool<E>(self, _v: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E>(self, _v: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E>(self, _v: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E>(self, _v: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E>(self, _v: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            if let Some(first_idx) = self.keys.iter().position(|k| *k == key.as_ref()) {
                let val = map.next_value::<StringifiedValue>()?.0;
                self.out[first_idx] = val;
                for idx in (first_idx + 1)..self.keys.len() {
                    if self.keys[idx] == key.as_ref() {
                        self.out[idx] = self.out[first_idx].clone();
                    }
                }
            } else {
                let _ = map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

/// 流式扫描顶层 JSON 对象，避免整棵 DOM 树堆分配；JSON 非法或非对象时将所有槽位清空。
fn extract_keys_from_slice(data: &[u8], keys: &[&str], out: &mut [String]) {
    if keys.is_empty() {
        return;
    }
    let res = {
        let mut de = serde_json::Deserializer::from_slice(data);
        de.deserialize_any(TopLevelObjectVisitor { keys, out })
            .and_then(|()| de.end())
    };
    if res.is_err() {
        out.fill_with(String::new);
    }
}

/// 从 JSON 字节切片中按有序 key 列表流式提取多个字段的字符串表示。
///
/// 具有以下行为：
/// - JSON 非法或根节点非对象时全部返回空串（长度与 `keys` 一致）
/// - 非字符串标量（数字/布尔）及嵌套结构转为文本表示
/// - 缺失字段或 `null` 值返回空串
/// - 未命中的字段零堆分配跳过
///
/// # 参数
/// * `data` - JSON 原始字节切片
/// * `keys` - 需要提取的顶层键名切片
///
/// # 返回值
/// * 与 `keys` 一一对应的字符串向量
///
/// # 示例
/// ```ignore
/// let json = br#"{"name": "John", "age": 30}"#;
/// let results = json_get_strings(json, &["name", "age", "missing"]);
/// assert_eq!(vec!["John", "30", ""], results);
/// ```
pub fn json_get_strings(data: &[u8], keys: &[&str]) -> Vec<String> {
    let mut out = vec![String::new(); keys.len()];
    extract_keys_from_slice(data, keys, &mut out);
    out
}

/// 从 JSON 字节切片中流式提取单个顶层字段的字符串表示。
///
/// 缺失字段、`null` 值或非法 JSON 统一返回空串，未命中的其余字段零堆分配跳过。
///
/// # 参数
/// * `data` - JSON 原始字节切片
/// * `key` - 待查找的顶层键名
///
/// # 返回值
/// * 对应的字符串值，或空串
///
/// # 示例
/// ```ignore
/// let json = br#"{"name": "John", "age": 30, "null_value": null}"#;
/// assert_eq!("John", json_get(json, "name"));
/// assert_eq!("30", json_get(json, "age"));
/// assert_eq!("", json_get(json, "null_value"));
/// assert_eq!("", json_get(json, "non_existent"));
/// ```
pub fn json_get(data: &[u8], key: &str) -> String {
    let mut out = [String::new()];
    extract_keys_from_slice(data, &[key], &mut out);
    let [first] = out;
    first
}

/// 从已解析的 JSON Map 中按 key 提取字符串表示。
///
/// # 参数
/// * `data` - 已解析的 JSON Map 引用
/// * `key` - 待查找的键名
///
/// # 返回值
/// * 对应的字符串值，缺失或 `null` 时返回空串
pub fn get_map_string(data: &Map<String, Value>, key: &str) -> String {
    data.get(key).map_or(String::new(), json_value_to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn json_get_returns_string_value_directly() {
        let json = br#"{"name": "John"}"#;
        assert_eq!(json_get(json, "name"), "John");
    }

    #[test]
    fn json_get_stringifies_non_string_values() {
        let json = br#"{"age": 30, "active": true}"#;
        assert_eq!(json_get(json, "age"), "30");
        assert_eq!(json_get(json, "active"), "true");
    }

    #[test]
    fn json_get_null_and_missing_return_empty() {
        let json = br#"{"x": null}"#;
        assert_eq!(json_get(json, "x"), "");
        assert_eq!(json_get(json, "nonexistent"), "");
    }

    #[test]
    fn json_get_invalid_json_returns_empty() {
        assert_eq!(json_get(b"not json", "any"), "");
    }

    #[test]
    fn json_get_strings_preserves_key_order_and_count() {
        let json = br#"{"name": "John", "age": 30}"#;
        assert_eq!(
            json_get_strings(json, &["name", "age", "missing"]),
            vec!["John".to_string(), "30".to_string(), String::new(),]
        );
    }

    #[test]
    fn json_get_strings_invalid_json_returns_all_empty() {
        // 长度仍与 keys 数量一致，调用方可放心按下标访问
        let out = json_get_strings(b"garbage", &["a", "b", "c"]);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|s| s.is_empty()));
    }

    #[test]
    fn streaming_extraction_handles_nested_duplicates_and_trailing_garbage() {
        let json =
            br#"{"ignored":{"big":[1,2,3]},"a":"first","ratio":1.0,"arr":[1,2],"a":"second"}"#;
        assert_eq!(json_get(json, "a"), "second");
        assert_eq!(json_get(json, "ratio"), "1.0");
        assert_eq!(json_get(json, "arr"), "[1,2]");
        assert_eq!(
            json_get_strings(json, &["a", "a", "ratio"]),
            vec![
                "second".to_string(),
                "second".to_string(),
                "1.0".to_string()
            ]
        );

        // 非对象根节点与尾随非法字节必须返回空
        assert_eq!(json_get(b"[1, 2, 3]", "0"), "");
        assert_eq!(json_get(br#"{"name":"John"} trailing"#, "name"), "");
    }
}
