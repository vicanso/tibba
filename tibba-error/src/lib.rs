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
    /// 是否对客户端隐去 `message` / `extra`；`None` 表示按状态码判定。
    ///
    /// `#[serde(skip)]`：这是「如何构造响应」的服务端控制位，不是响应内容本身，
    /// 不该出现在回给客户端的 JSON 里。
    #[serde(skip)]
    redact: Option<bool>,
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
    ///
    /// **只影响告警**，不影响响应体是否脱敏——脱敏见 [`Self::with_redact`]。
    #[must_use]
    pub fn with_exception(mut self, exception: bool) -> Self {
        self.data.exception = Some(exception);
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
        self.data.redact = Some(redact);
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

    /// 是否已显式设置脱敏；`None` 表示按状态码默认判定（见 [`Self::with_redact`]）。
    pub fn redact(&self) -> Option<bool> {
        self.data.redact
    }

    /// 附加上下文列表，未设置时返回空切片。
    pub fn extra(&self) -> &[String] {
        self.data.extra.as_deref().unwrap_or(&[])
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

/// 将 `Error` 转换为带 JSON 响应体和 `no-cache` 头的 HTTP 响应。
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        // 需脱敏时：响应体隐去可能含内部细节的原始 message 与 extra，只回通用文案；
        // 完整 message 仍随 self 存入 extensions 供服务端日志读取，不外泄给客户端。
        // category / sub_category / code 属分类信息，保留供前端处理。
        let mut res = if should_redact(status, self.data.redact) {
            let redacted = ErrorData {
                sub_category: self.data.sub_category.clone(),
                code: self.data.code.clone(),
                exception: self.data.exception,
                extra: None,
                // 服务端控制位，不参与序列化，取值无关紧要
                redact: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

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
        assert!(!should_redact(StatusCode::INTERNAL_SERVER_ERROR, Some(false)));
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
        assert_eq!(body_json(res).await["message"], "internal server error");

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
    fn no_cache_header_is_always_set() {
        let res = Error::new("x").with_status(400).into_response();
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );
    }

    /// `redact` 是服务端控制位，不得出现在回给客户端的 JSON 里。
    #[test]
    fn redact_flag_is_not_serialized() {
        let json = serde_json::to_string(&Error::new("m").with_redact(true)).unwrap();
        assert!(!json.contains("redact"), "序列化结果不应含 redact: {json}");
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
}
