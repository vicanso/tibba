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

//! 错误消息本地化：按 `Accept-Language` 协商语言，将应用错误响应的 `message` 翻译为对应语言。
//!
//! 设计要点——**零侵入**：
//! - 不改 [`tibba_error::Error`]，也不需在各错误构造处埋点；
//! - [`tibba_error::Error::into_response`] 会把 `Error` 放进响应扩展，本中间件据此识别
//!   「本应用的错误响应」，取其 `code` / `sub_category` / `category` 作为翻译 key 查目录，
//!   命中则只替换响应体里的 `message` 字段，其余（category / code 等）保持不变。
//!
//! 既有错误大多已带 `sub_category`（如 `too_many_requests` / `csrf_mismatch`），因此只要在
//! [`Catalog`] 里登记这些 key 的译文即可让它们「免改造」本地化。
//!
//! ## 用法
//! ```ignore
//! let catalog = Catalog::new("en")
//!     .add("en", "too_many_requests", "Too many requests, please retry later.")
//!     .add("zh", "too_many_requests", "请求过于频繁，请稍后再试。");
//! tibba_i18n::init(catalog).ok();
//! // 路由层挂中间件：.layer(from_fn(tibba_i18n::i18n))
//! ```

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;
use std::sync::OnceLock;
use tibba_error::Error as BaseError;

/// 本地化目录：`locale -> (key -> 模板)`。key 取错误的 code / sub_category / category。
pub struct Catalog {
    /// 协商不到受支持语言时回退的 locale（始终存在于 `locales` 中）。
    fallback: String,
    /// `locale -> (key -> 模板)`，locale 统一小写存储。
    locales: HashMap<String, HashMap<String, String>>,
}

impl Catalog {
    /// 以回退 locale 创建目录（`fallback` 自身也会被登记为受支持语言）。
    pub fn new(fallback: impl Into<String>) -> Self {
        let fallback = fallback.into().to_lowercase();
        let mut locales = HashMap::new();
        locales.insert(fallback.clone(), HashMap::new());
        Self { fallback, locales }
    }

    /// 添加一条翻译：`locale` + `key` -> `text`。链式调用。
    /// `locale` 统一转小写存储，以便与 `Accept-Language` 协商结果匹配。
    #[must_use]
    pub fn add(
        mut self,
        locale: impl Into<String>,
        key: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        self.locales
            .entry(locale.into().to_lowercase())
            .or_default()
            .insert(key.into(), text.into());
        self
    }

    /// 翻译 `key`，支持 `{name}` 占位符插值（`args` 为 name->value）。
    ///
    /// 先查 `locale`，缺失则回退 `fallback` locale；两者都没有该 key 时返回 `None`。
    pub fn translate(&self, locale: &str, key: &str, args: &[(&str, &str)]) -> Option<String> {
        let locale = locale.to_lowercase();
        let template = self
            .locales
            .get(&locale)
            .and_then(|m| m.get(key))
            .or_else(|| self.locales.get(&self.fallback).and_then(|m| m.get(key)))?;
        Some(interpolate(template, args))
    }

    /// 依据 `Accept-Language` 协商出受支持的 locale；无任何匹配时返回 `fallback`。
    pub fn negotiate(&self, accept_language: &str) -> String {
        negotiate_locale(accept_language, &self.fallback, |l| {
            self.locales.contains_key(l)
        })
    }
}

/// 用 `{name}` 占位符把 `args` 插入模板。无占位符时原样返回。
fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    if args.is_empty() {
        return template.to_string();
    }
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// 解析 `Accept-Language`（含 q 值），返回首个受支持的 locale；无则 `fallback`。
///
/// 解析规则：按 q 值降序遍历语言标签；标签本身受支持则直接采用，否则尝试主语言回退
/// （`zh-CN` 不支持时退到 `zh`）。`*` 通配符跳过。所有比较在小写下进行。
fn negotiate_locale(accept_language: &str, fallback: &str, supported: impl Fn(&str) -> bool) -> String {
    let mut tags: Vec<(String, f32)> = accept_language
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let mut it = part.split(';');
            let tag = it.next()?.trim().to_lowercase();
            if tag.is_empty() {
                return None;
            }
            // q 值缺省 1.0；非法 q 当作 1.0
            let q = it
                .find_map(|p| p.trim().strip_prefix("q=").and_then(|q| q.parse::<f32>().ok()))
                .unwrap_or(1.0);
            Some((tag, q))
        })
        .collect();
    // 稳定排序保证同 q 时维持原顺序（即客户端偏好顺序）
    tags.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (tag, _) in &tags {
        if tag == "*" {
            continue;
        }
        if supported(tag) {
            return tag.clone();
        }
        // 主语言回退：zh-cn -> zh
        if let Some((primary, _)) = tag.split_once('-')
            && supported(primary)
        {
            return primary.to_string();
        }
    }
    fallback.to_string()
}

/// 全局目录。`None` 表示未启用本地化（中间件直接透传）。
static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// 注册全局本地化目录（启动期一次）。已注册时返回 `Err(catalog)`。
pub fn init(catalog: Catalog) -> Result<(), Catalog> {
    CATALOG.set(catalog)
}

/// 取全局目录；未注册返回 `None`。
pub fn catalog() -> Option<&'static Catalog> {
    CATALOG.get()
}

/// i18n 中间件：按 `Accept-Language` 协商语言，把本应用错误响应的 `message` 本地化。
///
/// 仅作用于带 [`tibba_error::Error`] 扩展的响应（即经 `Error::into_response` 渲染的错误）；
/// 据错误的 `code` / `sub_category` / `category` 作为 key 查目录，命中则替换响应体的
/// `message`。未注册目录、无 `Accept-Language`、或无匹配翻译时**原样透传**，开销极小。
///
/// 译文仅替换 `message` 文本（用 `serde_json::Value` 局部改写，保留 category / code 等
/// 其它字段）。须挂在压缩层**内侧**，以对未压缩响应体改写。
pub async fn i18n(req: Request, next: Next) -> Response {
    let Some(catalog) = catalog() else {
        return next.run(req).await;
    };
    let accept = req
        .headers()
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let locale = catalog.negotiate(&accept);
    let mut res = next.run(req).await;

    // 仅本应用错误响应带 Error 扩展；非错误响应直接透传
    let Some(error) = res.extensions().get::<BaseError>().cloned() else {
        return res;
    };
    // key 优先级：显式 code > sub_category > category
    let key = error
        .code()
        .or_else(|| error.sub_category())
        .unwrap_or_else(|| error.category());
    if key.is_empty() {
        return res;
    }
    let Some(translated) = catalog.translate(&locale, key, &[]) else {
        return res;
    };
    // 译文与原文一致（例如协商到了原文所用语言）则无需改写
    if translated == error.message() {
        return res;
    }

    // 用 Value 仅替换 message，保留 category / code / sub_category 等其它字段
    let Ok(mut value) = serde_json::to_value(&error) else {
        return res;
    };
    if let Some(obj) = value.as_object_mut() {
        obj.insert("message".to_string(), serde_json::Value::String(translated));
    }
    let Ok(body) = serde_json::to_vec(&value) else {
        return res;
    };
    let len = body.len();
    *res.body_mut() = Body::from(body);
    res.headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn demo() -> Catalog {
        Catalog::new("en")
            .add("en", "too_many_requests", "Too many requests.")
            .add("zh", "too_many_requests", "请求过于频繁。")
            .add("en", "greeting", "Hello {name}.")
            .add("zh", "greeting", "你好 {name}。")
    }

    #[test]
    fn negotiate_prefers_higher_q() {
        // en 显式 q 较低，zh 较高 → 选 zh
        assert_eq!(demo().negotiate("en;q=0.8,zh;q=0.9"), "zh");
    }

    #[test]
    fn negotiate_primary_language_fallback() {
        // zh-CN 不在目录里，回退到主语言 zh
        assert_eq!(demo().negotiate("zh-CN,zh;q=0.9,en;q=0.8"), "zh");
    }

    #[test]
    fn negotiate_unsupported_returns_fallback() {
        // fr / de 都不支持 → 回退 fallback "en"
        assert_eq!(demo().negotiate("fr,de;q=0.5"), "en");
        // 空头也回退 fallback
        assert_eq!(demo().negotiate(""), "en");
    }

    #[test]
    fn translate_hits_locale_then_fallback() {
        let c = demo();
        assert_eq!(c.translate("zh", "too_many_requests", &[]).as_deref(), Some("请求过于频繁。"));
        // de 无此 locale → 回退 fallback en 的译文
        assert_eq!(
            c.translate("de", "too_many_requests", &[]).as_deref(),
            Some("Too many requests.")
        );
        // 未登记的 key → None
        assert_eq!(c.translate("zh", "unknown_key", &[]), None);
    }

    #[test]
    fn translate_interpolates_placeholders() {
        let c = demo();
        assert_eq!(
            c.translate("zh", "greeting", &[("name", "世界")]).as_deref(),
            Some("你好 世界。")
        );
    }
}
