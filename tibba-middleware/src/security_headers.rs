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

//! 安全响应头中间件 —— 为所有响应统一补齐常见安全头。
//!
//! ## 覆盖的头
//! - `Strict-Transport-Security`（HSTS）—— 强制 HTTPS，仅 HTTPS 下被浏览器尊重
//! - `X-Content-Type-Options: nosniff` —— 关闭 MIME 嗅探
//! - `X-Frame-Options` —— 防点击劫持（与 CSP frame-ancestors 互补）
//! - `Referrer-Policy` —— 控制 Referer 泄漏粒度
//! - `Content-Security-Policy`（CSP）—— 默认留空，按站点定制
//! - `Permissions-Policy` —— 默认留空，按需收敛浏览器特性
//!
//! ## 设计取舍
//! 所有头用 [`set_header_if_not_exist`] 写入：**handler 已显式设置的同名头优先**，
//! 中间件只补缺省值，不覆盖业务定制（如某接口需要 `X-Frame-Options: SAMEORIGIN`）。
//! 字段为空字符串时跳过该头，便于按部署环境裁剪。
//!
//! ## 用法
//! ```ignore
//! use tibba_middleware::{security_headers, SecurityHeaders};
//! use axum::middleware::from_fn_with_state;
//!
//! let config = SecurityHeaders::new().with_content_security_policy("default-src 'self'");
//! Router::new()
//!     .merge(api_router)
//!     .layer(from_fn_with_state(config, security_headers));
//! ```

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use tibba_util::set_header_if_not_exist;

/// 安全响应头配置。
///
/// 字段全部私有，只能通过链式 `with_xxx` 设置；空字符串表示「不输出该头」。
/// [`Default`] 给出适用于多数生产站点的安全基线，CSP / Permissions-Policy 因与
/// 前端资源强相关默认留空（错误的 CSP 会直接白屏），由调用方按站点显式开启。
#[derive(Debug, Clone)]
pub struct SecurityHeaders {
    /// `Strict-Transport-Security` 值；HTTP 明文请求下浏览器会忽略，故默认开启亦安全。
    hsts: String,
    /// `X-Content-Type-Options` 值，默认 `nosniff`。
    content_type_options: String,
    /// `X-Frame-Options` 值，默认 `DENY`。
    frame_options: String,
    /// `Referrer-Policy` 值，默认 `strict-origin-when-cross-origin`。
    referrer_policy: String,
    /// `Content-Security-Policy` 值，默认空。
    content_security_policy: String,
    /// `Permissions-Policy` 值，默认空。
    permissions_policy: String,
}

impl Default for SecurityHeaders {
    fn default() -> Self {
        Self {
            // 2 年 + 含子域 + preload，满足 hstspreload.org 提交门槛
            hsts: "max-age=63072000; includeSubDomains; preload".to_string(),
            content_type_options: "nosniff".to_string(),
            frame_options: "DENY".to_string(),
            referrer_policy: "strict-origin-when-cross-origin".to_string(),
            content_security_policy: String::new(),
            permissions_policy: String::new(),
        }
    }
}

impl SecurityHeaders {
    /// 以生产基线创建配置。等价于 [`SecurityHeaders::default`]。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置 `Strict-Transport-Security`。传空串可关闭该头。
    #[must_use]
    pub fn with_hsts(mut self, value: impl Into<String>) -> Self {
        self.hsts = value.into();
        self
    }

    /// 设置 `X-Content-Type-Options`。传空串可关闭该头。
    #[must_use]
    pub fn with_content_type_options(mut self, value: impl Into<String>) -> Self {
        self.content_type_options = value.into();
        self
    }

    /// 设置 `X-Frame-Options`（如 `SAMEORIGIN`）。传空串可关闭该头。
    #[must_use]
    pub fn with_frame_options(mut self, value: impl Into<String>) -> Self {
        self.frame_options = value.into();
        self
    }

    /// 设置 `Referrer-Policy`。传空串可关闭该头。
    #[must_use]
    pub fn with_referrer_policy(mut self, value: impl Into<String>) -> Self {
        self.referrer_policy = value.into();
        self
    }

    /// 设置 `Content-Security-Policy`。默认空，建议按站点资源显式配置。
    #[must_use]
    pub fn with_content_security_policy(mut self, value: impl Into<String>) -> Self {
        self.content_security_policy = value.into();
        self
    }

    /// 设置 `Permissions-Policy`（如 `geolocation=(), camera=()`）。
    #[must_use]
    pub fn with_permissions_policy(mut self, value: impl Into<String>) -> Self {
        self.permissions_policy = value.into();
        self
    }

    /// 把所有非空头补进响应（已存在的同名头不覆盖）。
    fn apply(&self, headers: &mut axum::http::HeaderMap) {
        // (header 名, 配置值) 一一对应；值为空则跳过。
        // set_header_if_not_exist 仅在 name/value 含非法字符时返回 Err，
        // 这里的值均为受控常量 / 配置，忽略错误不影响安全语义。
        let pairs = [
            ("strict-transport-security", &self.hsts),
            ("x-content-type-options", &self.content_type_options),
            ("x-frame-options", &self.frame_options),
            ("referrer-policy", &self.referrer_policy),
            ("content-security-policy", &self.content_security_policy),
            ("permissions-policy", &self.permissions_policy),
        ];
        for (name, value) in pairs {
            if !value.is_empty() {
                let _ = set_header_if_not_exist(headers, name, value);
            }
        }
    }
}

/// axum 中间件：在响应阶段补齐安全头。
///
/// **不会失败** —— 仅向响应追加头，非法值被静默跳过，保证请求链路不因此中断。
pub async fn security_headers(
    State(config): State<SecurityHeaders>,
    req: Request,
    next: Next,
) -> Response {
    let mut res = next.run(req).await;
    config.apply(res.headers_mut());
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    /// 默认配置补齐 4 个基线头，CSP / Permissions-Policy 默认不输出。
    #[test]
    fn default_applies_baseline_headers() {
        let mut headers = HeaderMap::new();
        SecurityHeaders::new().apply(&mut headers);

        assert_eq!(
            headers.get("strict-transport-security").unwrap(),
            "max-age=63072000; includeSubDomains; preload"
        );
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(
            headers.get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
        // 默认空的两个头不应出现
        assert!(headers.get("content-security-policy").is_none());
        assert!(headers.get("permissions-policy").is_none());
    }

    /// 已存在的同名头不被覆盖（handler 定制优先）。
    #[test]
    fn does_not_override_existing_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-frame-options", "SAMEORIGIN".parse().unwrap());
        SecurityHeaders::new().apply(&mut headers);

        assert_eq!(headers.get("x-frame-options").unwrap(), "SAMEORIGIN");
    }

    /// 链式设置 CSP 后该头被输出；空串关闭基线头。
    #[test]
    fn fluent_set_and_disable() {
        let mut headers = HeaderMap::new();
        SecurityHeaders::new()
            .with_content_security_policy("default-src 'self'")
            .with_hsts("")
            .apply(&mut headers);

        assert_eq!(
            headers.get("content-security-policy").unwrap(),
            "default-src 'self'"
        );
        // 传空串关闭 HSTS
        assert!(headers.get("strict-transport-security").is_none());
    }
}
