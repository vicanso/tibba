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
use axum::body::Body;
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::header::HeaderMap;
use axum::http::request::Parts;
use axum::http::{Request, header};
use serde::de::DeserializeOwned;
use tibba_error::Error;
use validator::Validate;

/// 参数提取 / 校验失败统一映射为 **400**。
///
/// 必须显式设置状态码：`Error::new` 的 `status` 默认是 0，`IntoResponse` 会把它
/// 回退成 500，而 5xx 又默认脱敏——于是「邮箱格式不对」这种纯客户端错误会以
/// `500 {"message":"internal server error"}` 的形式返回。调用方既拿不到可分流的
/// 状态码，也看不到是哪个字段不合法；监控侧还会把用户输错参数统计成服务端故障。
///
/// 三个 sub_category（`from_json` / `from_query` / `validate`）都是客户端输入问题，
/// 没有需要对外隐藏的内部细节，故一律 400 并保留原始 message。
fn map_err(err: impl ToString, sub_category: &str) -> Error {
    map_err_with_status(err, sub_category, 400)
}

/// 同 [`map_err`]，但使用给定的状态码（仍须是 4xx）。
fn map_err_with_status(err: impl ToString, sub_category: &str, status: u16) -> Error {
    Error::new(err)
        .with_category("params")
        .with_sub_category(sub_category)
        .with_status(status)
}

/// JSON 提取被拒时沿用 axum 给出的状态码。
///
/// axum 的 `JsonRejection` 本身就区分得很清楚：请求体超过 `DefaultBodyLimit`
/// 是 413、正文不是合法 JSON 是 400、JSON 合法但字段类型对不上是 422。此前一律
/// 压成 400，客户端没法区分「请求太大，别重试了」和「字段写错了」，网关侧的
/// 413 监控也永远是零。
fn json_rejection(err: axum::extract::rejection::JsonRejection) -> Error {
    let status = err.status().as_u16();
    // 防御：只认 4xx，axum 将来若返回别的类别也不至于把输入错误报成服务端故障
    let status = if (400..500).contains(&status) {
        status
    } else {
        400
    };
    map_err_with_status(err.body_text(), "from_json", status)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonParams<T>(pub T);

impl<T, S> FromRequest<S> for JsonParams<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        if json_content_type(req.headers()) {
            let Json(value) = Json::<T>::from_request(req, state)
                .await
                .map_err(json_rejection)?;
            value.validate().map_err(|e| map_err(e, "validate"))?;

            Ok(JsonParams(value))
        } else {
            // 415 Unsupported Media Type 才是这件事的准确状态码：请求本身可能完全
            // 合法，只是没按约定声明成 JSON
            Err(map_err_with_status(
                "expected request with `Content-Type: application/json`",
                "from_json",
                415,
            ))
        }
    }
}

/// 请求是否声明了 JSON 正文。
///
/// 走 mime 解析而非 `contains("application/json")`：后者只是子串匹配，
/// `application/jsonx` 会被误接受，而把 `application/json` 写进某个参数值的
/// `text/plain; fmt=application/json` 同样能蒙混过关——Content-Type 是不少
/// CSRF 判定的依据之一，不该用子串来认。
///
/// 接受 `application/json`（含任意参数，如 `; charset=utf-8`）与结构化语法后缀
/// `application/<x>+json`（如 `application/merge-patch+json`）。
fn json_content_type(headers: &HeaderMap) -> bool {
    let Some(value) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Ok(mime) = value.parse::<mime::Mime>() else {
        return false;
    };
    mime.type_() == mime::APPLICATION
        && (mime.subtype() == mime::JSON || mime.suffix() == Some(mime::JSON))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct QueryParams<T>(pub T);

impl<T, S> FromRequestParts<S> for QueryParams<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or_default();
        let params: T =
            serde_urlencoded::from_str(query).map_err(|err| map_err(err, "from_query"))?;
        params.validate().map_err(|e| map_err(e, "validate"))?;
        Ok(QueryParams(params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, StatusCode};
    use axum::response::IntoResponse;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Validate)]
    struct Payload {
        #[validate(email)]
        email: String,
    }

    fn json_request(body: &'static str) -> Request<Body> {
        Request::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("构造测试请求")
    }

    /// 取出拒绝响应的状态码与响应体。
    async fn reject(req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let err = JsonParams::<Payload>::from_request(req, &())
            .await
            .expect_err("本用例应当被拒绝");
        let res = err.into_response();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("读取响应体");
        (
            status,
            serde_json::from_slice(&bytes).expect("响应体应是合法 JSON"),
        )
    }

    /// **回归守卫**：校验失败必须是 400，且原因要如实回给调用方。
    ///
    /// 此前 `map_err` 不设状态码，`status` 停在 0 → `IntoResponse` 回退 500 →
    /// 5xx 默认脱敏，于是「邮箱格式不对」返回的是
    /// `500 {"message":"internal server error"}`。
    #[tokio::test]
    async fn validation_failure_is_client_error_with_reason() {
        let (status, body) = reject(json_request(r#"{"email":"not-an-email"}"#)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["category"], "params");
        assert_eq!(body["sub_category"], "validate");
        assert_ne!(
            body["message"], "internal server error",
            "校验失败的原因不得被脱敏掉"
        );
    }

    /// JSON 反序列化失败同样是客户端错误。
    #[tokio::test]
    async fn malformed_json_is_client_error() {
        let (status, body) = reject(json_request("{not json")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["sub_category"], "from_json");
    }

    /// 缺少 JSON content-type 是 415，而不是笼统的 400 / 500。
    #[tokio::test]
    async fn missing_content_type_is_unsupported_media_type() {
        let req = Request::builder()
            .body(Body::from(r#"{"email":"a@b.com"}"#))
            .expect("构造测试请求");
        let (status, body) = reject(req).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(body["sub_category"], "from_json");
    }

    /// JSON 合法但字段类型不对：沿用 axum 的 422，与「根本不是 JSON」（400）区分开。
    #[tokio::test]
    async fn type_mismatch_is_unprocessable_entity() {
        let (status, body) = reject(json_request(r#"{"email":123}"#)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["sub_category"], "from_json");
    }

    /// 请求体超过 `DefaultBodyLimit` 必须是 413——客户端据此知道「别重试了」。
    #[tokio::test]
    async fn oversized_body_is_payload_too_large() {
        use axum::extract::DefaultBodyLimit;
        use axum::routing::post;
        use tower::ServiceExt;

        async fn handler(JsonParams(_p): JsonParams<Payload>) -> &'static str {
            "ok"
        }
        let app = axum::Router::new()
            .route("/", post(handler))
            .layer(DefaultBodyLimit::max(64));
        let big = format!(r#"{{"email":"{}@b.com"}}"#, "a".repeat(256));
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(big))
                    .expect("构造测试请求"),
            )
            .await
            .expect("请求应得到响应");
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn json_content_type_accepts_json_family_only() {
        let check = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(value).expect("合法头部值"),
            );
            json_content_type(&headers)
        };

        assert!(check("application/json"));
        assert!(check("application/json; charset=utf-8"));
        assert!(check("APPLICATION/JSON"));
        // 结构化语法后缀
        assert!(check("application/merge-patch+json"));

        // 回归守卫：子串匹配会把下面这些一并放行
        assert!(!check("application/jsonx"));
        assert!(!check("text/plain; fmt=application/json"));
        assert!(!check("text/json"));
        assert!(!check("application/x-www-form-urlencoded"));
        assert!(!check("not a mime"));
        // 无 content-type
        assert!(!json_content_type(&HeaderMap::new()));
    }
}
