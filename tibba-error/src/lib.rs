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

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

/// 不常用的可选字段，装箱存放以控制 `Error` 的内存占用。
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ErrorData {
    /// 错误子分类，用于在同一 category 下进一步区分错误来源。
    pub sub_category: Option<String>,
    /// 业务错误码，供前端按码处理特定错误。
    pub code: Option<String>,
    /// 是否为需要告警的异常级错误。
    pub exception: Option<bool>,
    /// 附加信息列表，可携带多条上下文说明。
    pub extra: Option<Vec<String>>,
}

// 仅用于将 Error 序列化为扁平 JSON 对象的内部视图。
#[derive(Serialize)]
struct ErrorSerialize<'a> {
    category: &'a str,
    message: &'a str,
    #[serde(flatten)]
    data: &'a ErrorData,
}

// 仅用于从扁平 JSON 对象反序列化 Error 的内部视图。
#[derive(Deserialize)]
struct ErrorDeserialize {
    #[serde(default)]
    category: String,
    #[serde(default)]
    message: String,
    #[serde(flatten)]
    data: ErrorData,
}

/// 全局 HTTP 错误类型，贯穿整个应用。
///
/// `category` 与 `message` 始终存在，直接置于结构体上。
/// 可选字段通过 `Box<ErrorData>` 装箱，将 `Err` 变体保持在
/// `result_large_err` 的 128 字节限制以内。
#[derive(Debug, Clone, Default)]
pub struct Error {
    /// HTTP 状态码，0 表示未设置，`IntoResponse` 时回退为 500。
    pub status: u16,
    /// 错误来源模块或分类，如 "cache"、"db"。
    pub category: String,
    /// 面向用户或日志的错误描述信息。
    pub message: String,
    data: Box<ErrorData>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

/// 序列化为扁平 JSON 对象：`{ category, message, sub_category?, … }`。
impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorSerialize {
            category: &self.category,
            message: &self.message,
            data: &self.data,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Error {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let d = ErrorDeserialize::deserialize(deserializer)?;
        Ok(Self {
            status: 0,
            category: d.category,
            message: d.message,
            data: Box::new(d.data),
        })
    }
}

/// 通过 `Deref`/`DerefMut` 将 `ErrorData` 的可选字段（`sub_category`、
/// `code`、`exception`、`extra`）直接暴露在 `Error` 上，无需手动访问 `.data`。
impl Deref for Error {
    type Target = ErrorData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for Error {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Error {
    /// 以错误信息创建新的 `Error` 实例，其余字段均为默认值。
    #[must_use]
    pub fn new(message: impl ToString) -> Self {
        Self {
            message: message.to_string(),
            ..Default::default()
        }
    }

    /// 设置错误分类（模块来源），支持链式调用。
    #[must_use]
    pub fn with_category(mut self, category: impl ToString) -> Self {
        self.category = category.to_string();
        self
    }

    /// 设置错误子分类，支持链式调用。
    #[must_use]
    pub fn with_sub_category(mut self, sub_category: impl ToString) -> Self {
        self.sub_category = Some(sub_category.to_string());
        self
    }

    /// 设置业务错误码，支持链式调用。
    #[must_use]
    pub fn with_code(mut self, code: impl ToString) -> Self {
        self.code = Some(code.to_string());
        self
    }

    /// 设置 HTTP 状态码，支持链式调用。
    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// 标记是否为需要告警的异常级错误，支持链式调用。
    #[must_use]
    pub fn with_exception(mut self, exception: bool) -> Self {
        self.exception = Some(exception);
        self
    }

    /// 追加一条附加上下文信息，支持链式调用。
    #[must_use]
    pub fn add_extra(mut self, value: impl ToString) -> Self {
        self.extra
            .get_or_insert_with(Vec::new)
            .push(value.to_string());
        self
    }
}

/// 将 `Error` 转换为带 JSON 响应体和 `no-cache` 头的 HTTP 响应。
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        // 错误响应禁止缓存
        let mut res = (status, Json(&self)).into_response();
        res.extensions_mut().insert(self);
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        res
    }
}
