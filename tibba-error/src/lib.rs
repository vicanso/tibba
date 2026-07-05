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

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// 不常用的可选字段集合，装箱存放以控制 [`Error`] 的内存占用。
/// 仅作为 `Error` 内部实现，不对外暴露。
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
struct ErrorData {
    /// 错误子分类，用于在同一 category 下进一步区分错误来源。
    sub_category: Option<String>,
    /// 业务错误码，供前端按码处理特定错误。
    code: Option<String>,
    /// 是否为需要告警的异常级错误。
    exception: Option<bool>,
    /// 附加信息列表，可携带多条上下文说明。
    extra: Option<Vec<String>>,
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
/// 所有字段均为私有，必须通过 [`Error::new`] 创建并经由链式 `with_xxx` /
/// `add_xxx` 方法配置；读取使用同名 getter（[`Error::status`]、
/// [`Error::category`] 等）。
///
/// 内部把可选字段统一装箱到 `Box<ErrorData>`，将 `Result<_, Error>` 的
/// `Err` 变体保持在 `clippy::result_large_err` 128 字节限制以内。
#[derive(Debug, Clone, Default)]
pub struct Error {
    /// HTTP 状态码，0 表示未显式设置，`IntoResponse` 时回退为 500。
    status: u16,
    /// 错误来源模块或分类，如 "cache"、"db"。
    category: String,
    /// 面向用户或日志的错误描述信息。
    message: String,
    /// 不常用的可选字段，装箱以控制 `Error` 大小。
    data: Box<ErrorData>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
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

impl Error {
    /// 以错误信息创建新的 `Error` 实例，其余字段均为默认值。
    /// `message` 接受任意 `Display` 类型，便于直接由外部错误包装。
    #[must_use]
    pub fn new(message: impl ToString) -> Self {
        Self {
            message: message.to_string(),
            ..Default::default()
        }
    }

    // ---------- 链式 setter ----------

    /// 设置错误分类（模块来源），支持链式调用。
    #[must_use]
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// 设置错误子分类，支持链式调用。
    #[must_use]
    pub fn with_sub_category(mut self, sub_category: impl Into<String>) -> Self {
        self.data.sub_category = Some(sub_category.into());
        self
    }

    /// 设置业务错误码，支持链式调用。
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.data.code = Some(code.into());
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
        self.data.exception = Some(exception);
        self
    }

    /// 追加一条附加上下文信息，支持链式调用。
    #[must_use]
    pub fn add_extra(mut self, value: impl Into<String>) -> Self {
        self.data
            .extra
            .get_or_insert_with(Vec::new)
            .push(value.into());
        self
    }

    // ---------- getter ----------

    /// HTTP 状态码；返回 0 表示未显式设置，响应时会回退到 500。
    pub fn status(&self) -> u16 {
        self.status
    }

    /// 错误来源模块。
    pub fn category(&self) -> &str {
        &self.category
    }

    /// 面向用户或日志的错误描述。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 错误子分类，未设置时返回 `None`。
    pub fn sub_category(&self) -> Option<&str> {
        self.data.sub_category.as_deref()
    }

    /// 业务错误码，未设置时返回 `None`。
    pub fn code(&self) -> Option<&str> {
        self.data.code.as_deref()
    }

    /// 是否为需要告警的异常级错误；未显式设置时返回 `false`。
    pub fn is_exception(&self) -> bool {
        self.data.exception.unwrap_or(false)
    }

    /// 附加上下文列表，未设置时返回空切片。
    pub fn extra(&self) -> &[String] {
        self.data.extra.as_deref().unwrap_or(&[])
    }
}

/// 将 `Error` 转换为带 JSON 响应体和 `no-cache` 头的 HTTP 响应。
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        // 5xx / 异常级错误：响应体隐去可能含内部细节的原始 message 与 extra（如 sqlx / 库
        // 原始错误文本），只回通用文案；完整 message 仍随 self 存入 extensions 供服务端日志
        // 读取，不外泄给客户端。category / sub_category / code 属分类信息，保留供前端处理。
        let mut res = if status.is_server_error() || self.is_exception() {
            let redacted = ErrorData {
                sub_category: self.data.sub_category.clone(),
                code: self.data.code.clone(),
                exception: self.data.exception,
                extra: None,
            };
            (
                status,
                Json(ErrorSerialize {
                    category: &self.category,
                    message: "internal server error",
                    data: &redacted,
                }),
            )
                .into_response()
        } else {
            (status, Json(&self)).into_response()
        };
        // 把 Error 放入 extensions，方便日志/统计中间件读取上下文
        res.extensions_mut().insert(self);
        // 错误响应禁止缓存
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        res
    }
}
