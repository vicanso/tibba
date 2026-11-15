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
use std::borrow::Cow;

/// 不常用的可选字段集合，按需装箱存放以控制 [`Error`] 的内存占用并避免无谓堆分配。
/// 仅作为 `Error` 内部实现，不对外暴露。
#[derive(Debug, Default, Clone)]
struct ErrorData {
    /// 错误子分类，用于在同一 category 下进一步区分错误来源。
    sub_category: Option<String>,
    /// 业务错误码，供前端按码处理特定错误。
    code: Option<String>,
    /// 是否为需要告警的异常级错误。
    exception: Option<bool>,
    /// 附加信息列表，可携带多条上下文说明。
    extra: Option<Vec<String>>,
    /// 是否对客户端隐去 `message` / `extra`；`None` 表示按状态码判定。
    ///
    /// 这是「如何构造响应」的服务端控制位，不参与响应序列化。
    redact: Option<bool>,
}

// 仅用于将 Error 序列化为扁平 JSON 对象的内部视图（跨服务或日志持久化）。
#[derive(Serialize)]
struct ErrorSerialize<'a> {
    /// 未设置分类时不输出，与各可选字段的处理一致——
    /// `"category":""` 与缺省对前端没有区别，只是噪音。
    #[serde(skip_serializing_if = "str::is_empty")]
    category: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub_category: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exception: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<&'a [String]>,
}

// 仅用于面向 HTTP 客户端响应的 JSON 视图：永不输出服务端内部告警位 `exception`，
// 并在脱敏时隐去 `extra` 与原始 `message`。
#[derive(Serialize)]
struct ClientBody<'a> {
    #[serde(skip_serializing_if = "str::is_empty")]
    category: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub_category: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<&'a [String]>,
}

// 仅用于从扁平 JSON 对象反序列化 Error 的内部视图。
#[derive(Deserialize)]
struct ErrorDeserialize {
    #[serde(default)]
    category: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    sub_category: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    exception: Option<bool>,
    #[serde(default)]
    extra: Option<Vec<String>>,
}

/// 全局 HTTP 错误类型，贯穿整个应用。
///
/// 所有字段均为私有，必须通过 [`Error::new`] 创建并经由链式 `with_xxx` /
/// `add_xxx` 方法配置；读取使用同名 getter（[`Error::status`]、
/// [`Error::category`] 等）。
///
/// 内部把可选字段按需装箱到 `Option<Box<ErrorData>>`，常见错误零附加堆分配，
/// 同时将 `Result<_, Error>` 的 `Err` 变体保持在 `clippy::result_large_err`
/// 128 字节限制以内。
#[derive(Debug, Clone, Default)]
pub struct Error {
    /// HTTP 状态码，0 表示未显式设置，`IntoResponse` 时回退为 500。
    status: u16,
    /// 错误来源模块或分类，如 "cache"、"db"。
    category: String,
    /// 面向用户或日志的错误描述信息。
    message: String,
    /// 不常用的可选字段，按需装箱以控制 `Error` 大小与堆分配。
    data: Option<Box<ErrorData>>,
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
        let data = self.data.as_deref();
        ErrorSerialize {
            category: &self.category,
            message: &self.message,
            sub_category: data.and_then(|d| d.sub_category.as_deref()),
            code: data.and_then(|d| d.code.as_deref()),
            exception: data.and_then(|d| d.exception),
            extra: data.and_then(|d| d.extra.as_deref()),
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
        let data = if d.sub_category.is_none()
            && d.code.is_none()
            && d.exception.is_none()
            && d.extra.is_none()
        {
            None
        } else {
            Some(Box::new(ErrorData {
                sub_category: d.sub_category,
                code: d.code,
                exception: d.exception,
                extra: d.extra,
                redact: None,
            }))
        };
        Ok(Self {
            status: 0,
            category: d.category,
            message: d.message,
            data,
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

    fn data_mut(&mut self) -> &mut ErrorData {
        self.data.get_or_insert_with(Box::default)
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
        self.data_mut().sub_category = Some(sub_category.into());
        self
    }

    /// 设置业务错误码，支持链式调用。
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.data_mut().code = Some(code.into());
        self
    }

    /// 设置 HTTP 状态码，支持链式调用。
    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// 标记是否为需要告警的异常级错误，支持链式调用。
    ///
    /// **只影响告警**，不影响响应体是否脱敏——脱敏见 [`Self::with_redact`]。
    #[must_use]
    pub fn with_exception(mut self, exception: bool) -> Self {
        self.data_mut().exception = Some(exception);
        self
    }

    /// 显式控制是否对客户端隐去 `message` 与 `extra`，支持链式调用。
    ///
    /// 不调用本方法时按状态码判定：**5xx 隐去、4xx 保留**。
    ///
    /// 之所以要与 [`Self::with_exception`] 分开：两者是独立的轴。`exception`
    /// 的语义是「需要告警」，不等于「含内部细节」。此前二者被合并判定，
    /// 一个 `400 + exception` 的错误会回 `400 {"message":"internal server error"}`,
    /// 状态码与文案自相矛盾，前端无从处理。
    ///
    /// 需要覆盖默认时才用它，两个方向都支持：
    /// - `with_redact(true)`：4xx 但 message 含内部细节（如策略引擎的内部规则）
    /// - `with_redact(false)`：5xx 但 message 是可安全外露的固定文案
    #[must_use]
    pub fn with_redact(mut self, redact: bool) -> Self {
        self.data_mut().redact = Some(redact);
        self
    }

    /// 追加一条附加上下文信息，支持链式调用。
    #[must_use]
    pub fn add_extra(mut self, value: impl Into<String>) -> Self {
        self.data_mut()
            .extra
            .get_or_insert_with(Vec::new)
            .push(value.into());
        self
    }

    // ---------- getter ----------

    /// HTTP 状态码；返回 0 表示未显式设置，响应时会回退到 500。
    #[must_use]
    pub fn status(&self) -> u16 {
        self.status
    }

    /// 错误来源模块。
    #[must_use]
    pub fn category(&self) -> &str {
        &self.category
    }

    /// 面向用户或日志的错误描述。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 错误子分类，未设置时返回 `None`。
    #[must_use]
    pub fn sub_category(&self) -> Option<&str> {
        self.data.as_deref().and_then(|d| d.sub_category.as_deref())
    }

    /// 业务错误码，未设置时返回 `None`。
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.data.as_deref().and_then(|d| d.code.as_deref())
    }

    /// 是否为需要告警的异常级错误；未显式设置时返回 `false`。
    #[must_use]
    pub fn is_exception(&self) -> bool {
        self.data
            .as_deref()
            .and_then(|d| d.exception)
            .unwrap_or(false)
    }

    /// 是否已显式设置脱敏；`None` 表示按状态码默认判定（见 [`Self::with_redact`]）。
    #[must_use]
    pub fn redact(&self) -> Option<bool> {
        self.data.as_deref().and_then(|d| d.redact)
    }

    /// 返回按响应状态码与显式设置计算后的最终脱敏判定。
    #[must_use]
    pub fn should_redact(&self) -> bool {
        should_redact(response_status(self.status), self.redact())
    }

    /// 附加上下文列表，未设置时返回空切片。
    #[must_use]
    pub fn extra(&self) -> &[String] {
        self.data
            .as_deref()
            .and_then(|d| d.extra.as_deref())
            .unwrap_or(&[])
    }

    /// 构造对外响应 JSON 字节（遵循脱敏规则，且永不输出服务端 `exception` 标志）。
    /// `message_override` 非空时覆盖对外展示的 `message`（供 `tibba-i18n` 本地化使用）。
    pub fn to_client_json(
        &self,
        message_override: Option<&str>,
    ) -> std::result::Result<Vec<u8>, serde_json::Error> {
        let status = response_status(self.status);
        let redact = should_redact(status, self.redact());
        let fallback_msg = if redact {
            Some(redacted_message(status))
        } else {
            None
        };
        let message = match (message_override, fallback_msg.as_deref()) {
            (Some(m), _) => m,
            (None, Some(m)) => m,
            (None, None) => &self.message,
        };
        let body = ClientBody {
            category: &self.category,
            message,
            sub_category: self.sub_category(),
            code: self.code(),
            extra: if redact {
                None
            } else {
                self.data.as_deref().and_then(|d| d.extra.as_deref())
            },
        };
        serde_json::to_vec(&body)
    }

    /// 构造对外响应 JSON `Value`（遵循脱敏规则，且永不输出服务端 `exception` 标志）。
    #[must_use]
    pub fn to_client_payload(&self, message_override: Option<&str>) -> serde_json::Value {
        let status = response_status(self.status);
        let redact = should_redact(status, self.redact());
        let message = match message_override {
            Some(m) => Cow::Borrowed(m),
            None if redact => redacted_message(status),
            None => Cow::Borrowed(self.message.as_str()),
        };
        let mut map = serde_json::Map::new();
        if !self.category.is_empty() {
            map.insert(
                "category".to_string(),
                serde_json::Value::String(self.category.clone()),
            );
        }
        map.insert(
            "message".to_string(),
            serde_json::Value::String(message.into_owned()),
        );
        if let Some(sub) = self.sub_category() {
            map.insert(
                "sub_category".to_string(),
                serde_json::Value::String(sub.to_string()),
            );
        }
        if let Some(code) = self.code() {
            map.insert(
                "code".to_string(),
                serde_json::Value::String(code.to_string()),
            );
        }
        if !redact && let Some(extra) = self.data.as_deref().and_then(|d| d.extra.as_deref()) {
            map.insert(
                "extra".to_string(),
                serde_json::Value::Array(
                    extra
                        .iter()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }
        serde_json::Value::Object(map)
    }

    /// 由上游服务的 HTTP 错误响应重建 `Error`。
    ///
    /// 上游若是同样用本类型的 tibba 服务，`category` / `sub_category` / `code`
    /// 会被保留（大响应体丢弃 `extra`、超长 `message` 截断，且不盲信上游 `exception`
    /// 而按本方 `status >= 500` 重置）；否则退化为「把响应体当纯文本作 message」。
    ///
    /// # 为什么 status 从参数来而不是从响应体解析
    /// 状态码本来就在 HTTP 状态行上，把它再塞进 JSON body 是冗余——也正因如此
    /// `Error` 的序列化形式里没有它，单纯 `serde_json::from_slice` 得到的 Error
    /// 状态码是 0（落到 `IntoResponse` 会变成 500）。这个构造函数就是那块拼图：
    /// 由调用方把状态行上的值传进来，结构化字段从 body 来。
    ///
    /// # 注意：这是**上游的**状态码
    /// 返回的 `Error` 带的是上游给的状态码，不代表本服务该回什么。直接透传给
    /// 自己的客户端通常是错的（上游 401 不等于调用方未登录）。调用方应显式
    /// 决定对外状态码，见 `tibba-request` 的 `handle_fail` 如何映射成 502。
    ///
    /// 响应体不是本类型的 JSON 时，message 取响应体文本并截断到
    /// [`MAX_UPSTREAM_MESSAGE_LEN`] 字节——上游回一整页 HTML 错误页是常事，
    /// 整页塞进 message 只会把日志冲垮。
    #[must_use]
    pub fn from_upstream(status: u16, body: &[u8]) -> Self {
        const MAX_UPSTREAM_JSON_BYTES: usize = 64 * 1024;
        const MAX_UPSTREAM_EXTRA_BYTES: usize = 4 * 1024;

        if body.len() <= MAX_UPSTREAM_JSON_BYTES
            && let Ok(mut err) = serde_json::from_slice::<Self>(body)
            && !err.message.is_empty()
        {
            if err.message.len() > MAX_UPSTREAM_MESSAGE_LEN {
                err.message = truncate_str_to_max(&err.message);
            }
            if let Some(data) = err.data.as_deref_mut() {
                if body.len() > MAX_UPSTREAM_EXTRA_BYTES {
                    data.extra = None;
                }
                // 不信任外部上游 JSON 中的 `exception` 告警标记，统一按状态码重置
                data.exception = if status >= 500 { Some(true) } else { None };
            } else if status >= 500 {
                err = err.with_exception(true);
            }
            return err.with_status(status);
        }
        Self::new(truncate_upstream_body(body)).with_status(status)
    }
}

/// 非结构化上游响应体转成 message 时的长度上限（字节）。
pub const MAX_UPSTREAM_MESSAGE_LEN: usize = 512;

/// 将字符串按字符边界截断到 [`MAX_UPSTREAM_MESSAGE_LEN`] 字节并追加省略号。
fn truncate_str_to_max(text: &str) -> String {
    if text.len() <= MAX_UPSTREAM_MESSAGE_LEN {
        return text.to_string();
    }
    let end = text.floor_char_boundary(MAX_UPSTREAM_MESSAGE_LEN);
    format!("{}…", &text[..end])
}

/// 把上游响应体截成一条能进日志的 message。
///
/// 先裁剪首尾 ASCII 空白并限制 decode 窗口，再按字符边界截断：
/// 既避免对数兆响应体做全量 UTF-8 解码，也避免切在多字节字符中间。
fn truncate_upstream_body(body: &[u8]) -> String {
    let trimmed_bytes = body.trim_ascii();
    if trimmed_bytes.is_empty() {
        return "upstream returned an empty body".to_string();
    }
    let max_decode = MAX_UPSTREAM_MESSAGE_LEN * 4;
    let slice = &trimmed_bytes[..trimmed_bytes.len().min(max_decode)];
    let text = String::from_utf8_lossy(slice);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "upstream returned an empty body".to_string();
    }
    if trimmed.len() <= MAX_UPSTREAM_MESSAGE_LEN && trimmed_bytes.len() <= slice.len() {
        return trimmed.to_string();
    }
    let end = trimmed.floor_char_boundary(MAX_UPSTREAM_MESSAGE_LEN);
    format!("{}…", &trimmed[..end])
}

/// 把 `Error` 上记录的状态码规整成**错误类**响应状态码。
///
/// `StatusCode::from_u16` 接受 100..=999 的任何值，于是 `with_status(200)` 的
/// Error 会以 `200 OK` 发出、带着一个错误 body——客户端按状态码判断成功，
/// 错误被静默吞掉；`with_status(600)` 这类越界值也会原样上线。错误响应只能
/// 是 4xx / 5xx，其余一律（含未设置的 0）回退为 500。
fn response_status(status: u16) -> StatusCode {
    match StatusCode::from_u16(status) {
        Ok(code) if code.is_client_error() || code.is_server_error() => code,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// 脱敏后对外的通用文案：取状态码的标准短语（小写），与状态码保持一致。
///
/// 常见状态码直接返回静态借用切片，零堆分配。
fn redacted_message(status: StatusCode) -> Cow<'static, str> {
    match status {
        StatusCode::INTERNAL_SERVER_ERROR => Cow::Borrowed("internal server error"),
        StatusCode::BAD_GATEWAY => Cow::Borrowed("bad gateway"),
        StatusCode::SERVICE_UNAVAILABLE => Cow::Borrowed("service unavailable"),
        StatusCode::GATEWAY_TIMEOUT => Cow::Borrowed("gateway timeout"),
        StatusCode::UNAUTHORIZED => Cow::Borrowed("unauthorized"),
        StatusCode::FORBIDDEN => Cow::Borrowed("forbidden"),
        StatusCode::NOT_FOUND => Cow::Borrowed("not found"),
        StatusCode::TOO_MANY_REQUESTS => Cow::Borrowed("too many requests"),
        _ => match status.canonical_reason() {
            Some(reason) => Cow::Owned(reason.to_ascii_lowercase()),
            None if status.is_client_error() => Cow::Borrowed("client error"),
            None => Cow::Borrowed("internal server error"),
        },
    }
}

/// 依据状态码与显式设置，判定响应体是否需要隐去 `message` / `extra`。
///
/// 默认只看状态码：5xx 的 message 多半是 sqlx / 底层库的原始错误文本，不能外泄；
/// 4xx 是给调用方看的业务信息，必须保留，否则前端拿不到可处理的原因。
///
/// 注意这里**不再**参考 `exception`。它只表示「需要告警」，与「含内部细节」无关，
/// 详见 [`Error::with_redact`]。
fn should_redact(status: StatusCode, explicit: Option<bool>) -> bool {
    explicit.unwrap_or_else(|| status.is_server_error())
}

/// 将 `Error` 转换为带 JSON 响应体和 `no-store` 头的 HTTP 响应。
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = response_status(self.status);
        let redact = should_redact(status, self.redact());
        // 需脱敏时：响应体隐去可能含内部细节的原始 message 与 extra，只回通用文案；
        // 且所有客户端响应均不序列化服务端告警位 `exception`。
        // 完整 Error 仍存入 extensions 供服务端日志/告警中间件读取。
        let redacted_msg = if redact {
            Some(redacted_message(status))
        } else {
            None
        };
        let message = redacted_msg.as_deref().unwrap_or(&self.message);
        let mut res = (
            status,
            Json(ClientBody {
                category: &self.category,
                message,
                sub_category: self.sub_category(),
                code: self.code(),
                extra: if redact {
                    None
                } else {
                    self.data.as_deref().and_then(|d| d.extra.as_deref())
                },
            }),
        )
            .into_response();
        // 把 Error 放入 extensions，方便日志/统计中间件读取上下文
        res.extensions_mut().insert(self);
        // 错误响应禁止缓存。用 no-store 而非 no-cache：后者只要求「用前先验证」，
        // 仍允许浏览器 / 代理把响应**存下来**；错误体里常有账号、资源 ID 之类的
        // 上下文，不该落进任何缓存。
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// 未设分类时不输出空的 `category`，且仍能反序列化回来。
    #[test]
    fn empty_category_is_omitted_and_round_trips() {
        let json = serde_json::to_string(&Error::new("m")).expect("序列化");
        assert_eq!(json, r#"{"message":"m"}"#);
        let back: Error = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(back.category(), "");
        assert_eq!(back.message(), "m");
    }

    /// 取出响应体 JSON，供断言脱敏与否。
    async fn body_json(res: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("读取响应体");
        serde_json::from_slice(&bytes).expect("响应体应是合法 JSON")
    }

    #[test]
    fn redact_defaults_to_status_class_only() {
        // 默认：5xx 脱敏、4xx 不脱敏
        assert!(should_redact(StatusCode::INTERNAL_SERVER_ERROR, None));
        assert!(should_redact(StatusCode::SERVICE_UNAVAILABLE, None));
        assert!(!should_redact(StatusCode::BAD_REQUEST, None));
        assert!(!should_redact(StatusCode::NOT_FOUND, None));
        assert!(!should_redact(StatusCode::OK, None));
        // 显式设置双向覆盖默认
        assert!(should_redact(StatusCode::BAD_REQUEST, Some(true)));
        assert!(!should_redact(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(false)
        ));
    }

    /// 回归守卫：`exception` 只管告警，不得触发脱敏。
    ///
    /// 此前判定是 `status.is_server_error() || is_exception()`，于是
    /// 400 + exception 会回 `{"message":"internal server error"}` —— 状态码
    /// 与文案自相矛盾，前端无从处理。
    #[tokio::test]
    async fn exception_on_4xx_keeps_client_facing_message() {
        let err = Error::new("email format invalid")
            .with_category("params")
            .with_status(400)
            .with_exception(true);
        let res = err.into_response();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        let body = body_json(res).await;
        assert_eq!(body["message"], "email format invalid");
        assert_eq!(body["category"], "params");
    }

    #[tokio::test]
    async fn server_error_message_is_redacted() {
        let err = Error::new("relation \"users\" does not exist")
            .with_category("db")
            .with_sub_category("sqlx")
            .with_status(500)
            .add_extra("connection=primary");
        let res = err.into_response();

        let body = body_json(res).await;
        assert_eq!(body["message"], "internal server error");
        // 分类信息保留供前端分流
        assert_eq!(body["category"], "db");
        assert_eq!(body["sub_category"], "sqlx");
        // extra 可能含内部上下文，必须一并隐去
        assert!(body["extra"].is_null());
        // 原始 message 不得以任何形式出现在响应体里
        assert!(!body.to_string().contains("does not exist"));
    }

    /// 未设 status 时回退 500，同样按 5xx 脱敏（status=0 不是 4xx）。
    #[tokio::test]
    async fn unset_status_falls_back_to_redacted_500() {
        let err = Error::new("raw internal detail").with_category("cache");
        let res = err.into_response();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body_json(res).await["message"], "internal server error");
    }

    #[tokio::test]
    async fn explicit_redact_overrides_both_directions() {
        // 4xx 但强制脱敏
        let res = Error::new("internal policy rule #42 denied")
            .with_status(403)
            .with_redact(true)
            .into_response();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        // 脱敏文案与状态码一致，而不是「403 + internal server error」
        assert_eq!(body_json(res).await["message"], "forbidden");

        // 5xx 但确认文案安全，照常回给客户端
        let res = Error::new("upstream is warming up, retry later")
            .with_status(503)
            .with_redact(false)
            .into_response();
        assert_eq!(
            body_json(res).await["message"],
            "upstream is warming up, retry later"
        );
    }

    /// 完整 Error 始终进 extensions，供日志中间件读取未脱敏的原文。
    #[test]
    fn full_error_is_preserved_in_extensions() {
        let res = Error::new("secret detail").with_status(500).into_response();
        let stored = res
            .extensions()
            .get::<Error>()
            .expect("Error 应存入 extensions");
        assert_eq!(stored.message(), "secret detail");
    }

    #[test]
    fn no_store_header_is_always_set() {
        let res = Error::new("x").with_status(400).into_response();
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }

    /// **回归守卫**：非错误类状态码不得原样发出。
    ///
    /// `StatusCode::from_u16` 接受 100..=999，此前 `with_status(200)` 的 Error
    /// 会回 `200 OK` + 错误 body，客户端按状态码判为成功，错误被静默吞掉。
    #[test]
    fn non_error_status_falls_back_to_500() {
        for bogus in [0, 99, 100, 200, 204, 302, 399, 600, 999] {
            let res = Error::new("x").with_status(bogus).into_response();
            assert_eq!(
                res.status(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "status={bogus} 应回退为 500"
            );
        }
        // 合法的 4xx / 5xx 原样保留
        for ok in [400, 404, 418, 429, 499, 500, 502, 503, 599] {
            let res = Error::new("x").with_status(ok).into_response();
            assert_eq!(res.status().as_u16(), ok);
        }
    }

    /// 脱敏文案随状态码走，未登记的状态码按大类兜底。
    #[test]
    fn redacted_message_matches_status_class() {
        assert_eq!(
            redacted_message(StatusCode::INTERNAL_SERVER_ERROR),
            "internal server error"
        );
        assert_eq!(
            redacted_message(StatusCode::SERVICE_UNAVAILABLE),
            "service unavailable"
        );
        assert_eq!(redacted_message(StatusCode::FORBIDDEN), "forbidden");
        // 无标准短语的码：4xx 不能说成「服务器内部错误」
        let unknown_4xx = StatusCode::from_u16(499).expect("合法状态码");
        assert_eq!(redacted_message(unknown_4xx), "client error");
        let unknown_5xx = StatusCode::from_u16(599).expect("合法状态码");
        assert_eq!(redacted_message(unknown_5xx), "internal server error");
    }

    /// `redact` 是服务端控制位，不得出现在回给客户端的 JSON 里。
    #[test]
    fn redact_flag_is_not_serialized() {
        let json = serde_json::to_string(&Error::new("m").with_redact(true)).unwrap();
        assert!(!json.contains("redact"), "序列化结果不应含 redact: {json}");
    }

    /// 未设置的可选字段不得出现在响应体里（此前恒为 `"code":null` 等噪音）。
    #[tokio::test]
    async fn unset_optional_fields_are_omitted() {
        let res = Error::new("bad email")
            .with_category("params")
            .with_status(400)
            .into_response();
        let body = body_json(res).await;
        let obj = body.as_object().expect("响应体应是 JSON 对象");

        assert_eq!(obj.len(), 2, "只应剩 category / message，实际: {obj:?}");
        assert_eq!(obj["category"], "params");
        assert!(!obj.contains_key("code"));
        assert!(!obj.contains_key("exception"));
        assert!(!obj.contains_key("extra"));
        assert!(!obj.contains_key("sub_category"));
    }

    /// 脱敏响应不得外泄 `exception`——它只表示服务端要不要告警。
    #[tokio::test]
    async fn redacted_response_hides_exception_flag() {
        let res = Error::new("sqlx: connection refused")
            .with_category("db")
            .with_status(500)
            .with_exception(true)
            .into_response();
        let body = body_json(res).await;

        assert_eq!(body["message"], "internal server error");
        assert!(
            body.get("exception").is_none(),
            "脱敏响应仍暴露了告警分级: {body}"
        );
        // 服务端侧必须仍拿得到，否则 tracker 无法告警
    }

    /// 与上一条配套：脱敏不影响服务端读取 `exception`。
    #[test]
    fn redacted_response_still_carries_exception_in_extensions() {
        let res = Error::new("boom")
            .with_status(500)
            .with_exception(true)
            .into_response();
        let stored = res
            .extensions()
            .get::<Error>()
            .expect("Error 应存入 extensions");
        assert!(stored.is_exception());
    }

    /// 上游是同用本类型的 tibba 服务时，结构化字段必须一路带回来。
    ///
    /// 此前跨服务只能靠 `json_get(body, "message")` 取一个字符串，
    /// category / sub_category / code / extra 全丢，status 也归 0（落地成 500）。
    #[test]
    fn from_upstream_restores_structured_fields() {
        let upstream = Error::new("account is locked")
            .with_category("user")
            .with_sub_category("locked")
            .with_code("E4031")
            .add_extra("retry_after=600")
            .with_status(403);
        let body = serde_json::to_vec(&upstream).expect("序列化上游错误");

        let restored = Error::from_upstream(403, &body);
        assert_eq!(restored.message(), "account is locked");
        assert_eq!(restored.category(), "user");
        assert_eq!(restored.sub_category(), Some("locked"));
        assert_eq!(restored.code(), Some("E4031"));
        assert_eq!(restored.extra(), ["retry_after=600".to_string()]);
        // 状态码来自状态行参数，补上了序列化丢失的那一块
        assert_eq!(restored.status(), 403);
    }

    /// 上游不是 tibba 服务时退化为纯文本，且仍带上状态码。
    #[test]
    fn from_upstream_falls_back_to_plain_text() {
        let err = Error::from_upstream(502, b"  <html>Bad Gateway</html>  ");
        assert_eq!(err.message(), "<html>Bad Gateway</html>");
        assert_eq!(err.status(), 502);

        // 合法 JSON 但不是本类型（没有 message 字段）同样走回退
        let err = Error::from_upstream(400, br#"{"error":"invalid_grant"}"#);
        assert_eq!(err.status(), 400);
        assert!(err.message().contains("invalid_grant"));

        // 空响应体
        let err = Error::from_upstream(504, b"");
        assert_eq!(err.status(), 504);
        assert!(!err.message().is_empty(), "空 body 也要有可读的 message");
    }

    /// 上游回一整页 HTML 是常事，整页塞进 message 会把日志冲垮。
    #[test]
    fn from_upstream_truncates_oversized_body() {
        let body = "x".repeat(MAX_UPSTREAM_MESSAGE_LEN * 4);
        let err = Error::from_upstream(500, body.as_bytes());
        assert!(
            err.message().len() <= MAX_UPSTREAM_MESSAGE_LEN + 4,
            "实际长度 {}",
            err.message().len()
        );
        assert!(err.message().ends_with('…'));
    }

    /// 截断必须按字符边界，切在多字节字符中间会产生非法 UTF-8。
    #[test]
    fn from_upstream_truncation_respects_char_boundaries() {
        // 每个汉字 3 字节，上限不是 3 的倍数，必然切在字符中间
        let body = "错".repeat(MAX_UPSTREAM_MESSAGE_LEN);
        let err = Error::from_upstream(500, body.as_bytes());
        // 能取到 &str 本身就说明没切坏；再确认确实被截断了
        assert!(err.message().chars().all(|c| c == '错' || c == '…'));
        assert!(err.message().len() <= MAX_UPSTREAM_MESSAGE_LEN + 4);
    }

    #[test]
    fn serde_round_trip_preserves_public_fields() {
        let err = Error::new("boom")
            .with_category("db")
            .with_sub_category("sqlx")
            .with_code("E1001")
            .with_exception(true)
            .add_extra("a")
            .add_extra("b");
        let json = serde_json::to_string(&err).unwrap();
        let back: Error = serde_json::from_str(&json).unwrap();

        assert_eq!(back.message(), "boom");
        assert_eq!(back.category(), "db");
        assert_eq!(back.sub_category(), Some("sqlx"));
        assert_eq!(back.code(), Some("E1001"));
        assert!(back.is_exception());
        assert_eq!(back.extra(), ["a".to_string(), "b".to_string()]);
        // status 不参与序列化，跨服务传递会丢失（既有行为）
        assert_eq!(back.status(), 0);
    }

    #[test]
    fn error_size_stays_within_limit_and_avoids_heap_alloc_when_simple() {
        assert!(std::mem::size_of::<Error>() <= 64);
        let simple = Error::new("simple");
        assert!(simple.data.is_none());
    }

    #[tokio::test]
    async fn non_redacted_response_never_leaks_exception_flag() {
        let res = Error::new("email invalid")
            .with_category("params")
            .with_status(400)
            .with_exception(true)
            .add_extra("field=email")
            .into_response();
        let body = body_json(res).await;
        assert_eq!(body["message"], "email invalid");
        assert_eq!(body["extra"], serde_json::json!(["field=email"]));
        assert!(body.get("exception").is_none());
    }

    #[test]
    fn from_upstream_whitespace_body_and_untrusted_exception_reset() {
        let err = Error::from_upstream(502, b"   \r\n\t  ");
        assert_eq!(err.message(), "upstream returned an empty body");

        let upstream = Error::new("bad request")
            .with_exception(true)
            .add_extra("detail");
        let body = serde_json::to_vec(&upstream).unwrap();
        let restored = Error::from_upstream(400, &body);
        assert!(!restored.is_exception(), "400 上游不应盲信 exception=true");

        // 超长结构化 message 也需截断，超大 body 丢弃 extra
        let huge_extra = "e".repeat(5000);
        let huge_msg = "m".repeat(MAX_UPSTREAM_MESSAGE_LEN * 2);
        let json_body = serde_json::to_vec(&serde_json::json!({
            "category": "svc",
            "message": huge_msg,
            "extra": [huge_extra],
        }))
        .unwrap();
        let bounded = Error::from_upstream(500, &json_body);
        assert!(bounded.message().len() <= MAX_UPSTREAM_MESSAGE_LEN + 4);
        assert!(bounded.message().ends_with('…'));
        assert!(bounded.extra().is_empty());
        assert!(bounded.is_exception());
    }

    #[test]
    fn to_client_payload_respects_redaction_and_override() {
        let err = Error::new("secret db detail")
            .with_category("db")
            .with_sub_category("sqlx")
            .with_code("DB_ERR")
            .with_status(500)
            .with_exception(true)
            .add_extra("pg=primary");
        assert!(err.should_redact());
        let payload = err.to_client_payload(Some("系统繁忙"));
        assert_eq!(payload["message"], "系统繁忙");
        assert_eq!(payload["code"], "DB_ERR");
        assert!(payload.get("extra").is_none());
        assert!(payload.get("exception").is_none());
    }
}
