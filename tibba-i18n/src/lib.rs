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

/// 最大解析的 `Accept-Language` 语言项数量，防恶意超长头造成无界分配/排序。
const MAX_LANGUAGE_RANGES: usize = 16;

/// 用 `{name}` 占位符单遍扫描把 `args` 插入模板，避免多次 `replace` 重扫与替换值二次展开。
fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    if args.is_empty() || !template.contains('{') {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find('}') {
            let key = &after_open[..close];
            if let Some((_, val)) = args.iter().find(|(k, _)| *k == key) {
                out.push_str(val);
            } else {
                out.push('{');
                out.push_str(key);
                out.push('}');
            }
            rest = &after_open[close + 1..];
        } else {
            out.push('{');
            rest = after_open;
        }
    }
    out.push_str(rest);
    out
}

/// 按 RFC 9110 §12.4.2 解析 qvalue（`0`..`1` 最多三位小数），转换为毫分比整数 `0..=1000`。
fn parse_qvalue(raw: &str) -> Option<u16> {
    let raw = raw.trim();
    let mut parts = raw.split('.');
    let int_part = parts.next()?;
    let frac_part = parts.next();
    if parts.next().is_some() {
        return None;
    }
    match int_part {
        "1" => {
            if let Some(frac) = frac_part
                && (frac.is_empty() || frac.len() > 3 || !frac.bytes().all(|b| b == b'0'))
            {
                return None;
            }
            Some(1000)
        }
        "0" => match frac_part {
            None => Some(0),
            Some(frac)
                if !frac.is_empty()
                    && frac.len() <= 3
                    && frac.bytes().all(|b| b.is_ascii_digit()) =>
            {
                let mut val = 0u16;
                let bytes = frac.as_bytes();
                for i in 0..3 {
                    let digit = if i < bytes.len() {
                        u16::from(bytes[i] - b'0')
                    } else {
                        0
                    };
                    val = val * 10 + digit;
                }
                Some(val)
            }
            _ => None,
        },
        _ => None,
    }
}

/// 按 RFC 9110 解析 `Accept-Language`，返回首个受支持的 locale；无则 `fallback`。
///
/// 规则：
/// - 最多解析前 16 个语言项；
/// - 大小写无关解析 `q=`/`Q=` 权重并转 `0..=1000` 毫分比稳定降序排列（避免浮点排序异常）；
/// - `q=0` 显式拒绝该标签（不参与匹配，也不被通配/前缀回退命中）；
/// - 多段语言标签按 `rsplit_once('-')` 逐级回退（如 `zh-hant-hk` → `zh-hant` → `zh`）。
fn negotiate_locale(
    accept_language: &str,
    fallback: &str,
    supported: impl Fn(&str) -> bool,
) -> String {
    let mut rejected: Vec<String> = Vec::new();
    let mut tags: Vec<(String, u16)> = Vec::new();

    for part in accept_language.split(',').take(MAX_LANGUAGE_RANGES) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut it = part.split(';');
        let Some(raw_tag) = it.next() else {
            continue;
        };
        let tag = raw_tag.trim().to_ascii_lowercase();
        if tag.is_empty() {
            continue;
        }
        let mut q: Option<u16> = Some(1000);
        for param in it {
            let param = param.trim();
            if let Some((k, v)) = param.split_once('=')
                && k.trim().eq_ignore_ascii_case("q")
            {
                q = parse_qvalue(v);
                break;
            }
        }
        let Some(q) = q else {
            continue;
        };
        if q == 0 {
            rejected.push(tag);
        } else {
            tags.push((tag, q));
        }
    }

    // 稳定降序：同 q 维持客户端声明顺序
    tags.sort_by_key(|a| std::cmp::Reverse(a.1));

    for (tag, _) in &tags {
        if tag == "*" {
            continue;
        }
        let mut cur = tag.as_str();
        loop {
            if !rejected.iter().any(|r| r == cur) && supported(cur) {
                return cur.to_string();
            }
            match cur.rsplit_once('-') {
                Some((parent, _)) => cur = parent,
                None => break,
            }
        }
    }
    if rejected.iter().any(|r| r == fallback) {
        return fallback.to_string();
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
    let accept_hdr = req.headers().get(header::ACCEPT_LANGUAGE).cloned();
    let res = next.run(req).await;
    // 先检查响应扩展：非错误响应直接透传，避免热路径解析 Accept-Language 与分配字符串
    if res.extensions().get::<BaseError>().is_none() {
        return res;
    }
    let accept = accept_hdr
        .as_ref()
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    localize_response(catalog, accept, res)
}

fn append_vary_accept_language(headers: &mut axum::http::HeaderMap) {
    if let Some(existing) = headers.get(header::VARY)
        && let Ok(val) = existing.to_str()
    {
        let already = val
            .split(',')
            .any(|v| v.trim().eq_ignore_ascii_case("accept-language") || v.trim() == "*");
        if already {
            return;
        }
        if let Ok(joined) = HeaderValue::from_str(&format!("{val}, Accept-Language")) {
            headers.insert(header::VARY, joined);
            return;
        }
    }
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Language"));
}

/// 使用协商后的语言目录本地化错误响应体。
fn localize_response(catalog: &Catalog, accept: &str, mut res: Response) -> Response {
    // 仅本应用错误响应带 Error 扩展；非错误响应直接透传
    let Some(error) = res.extensions().get::<BaseError>().cloned() else {
        return res;
    };
    // 已脱敏响应若未显式设置业务 code，不覆盖其通用脱敏文案
    if error.should_redact() && error.code().is_none() {
        return res;
    }
    let locale = catalog.negotiate(accept);
    // 查找优先级：code > "{category}.{sub_category}" > sub_category > category
    let translated = if let Some(code) = error.code() {
        catalog.translate(&locale, code, &[])
    } else if let Some(sub) = error.sub_category() {
        if !error.category().is_empty() {
            let scoped = format!("{}.{sub}", error.category());
            catalog
                .translate(&locale, &scoped, &[])
                .or_else(|| catalog.translate(&locale, sub, &[]))
                .or_else(|| catalog.translate(&locale, error.category(), &[]))
        } else {
            catalog.translate(&locale, sub, &[])
        }
    } else if !error.category().is_empty() {
        catalog.translate(&locale, error.category(), &[])
    } else {
        None
    };

    let Some(translated) = translated else {
        return res;
    };
    append_vary_accept_language(res.headers_mut());
    // 译文与原文一致且非脱敏改写则无需重序列化
    if !error.should_redact() && translated == error.message() {
        return res;
    }

    // 经由 to_client_json 统一输出对外响应：严格遵守脱敏规则，永不泄露 extra/exception
    let Ok(body) = error.to_client_json(Some(&translated)) else {
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
        assert_eq!(
            c.translate("zh", "too_many_requests", &[]).as_deref(),
            Some("请求过于频繁。")
        );
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
            c.translate("zh", "greeting", &[("name", "世界")])
                .as_deref(),
            Some("你好 世界。")
        );
    }

    /// 轮询即时就绪的内存 Response body。
    fn ready_body_json(res: Response) -> serde_json::Value {
        use std::future::Future;
        use std::task::{Context, Poll, Waker};
        let mut fut = std::pin::pin!(axum::body::to_bytes(res.into_body(), usize::MAX));
        let mut cx = Context::from_waker(Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(bytes)) => serde_json::from_slice(&bytes).unwrap(),
            other => panic!("unexpected poll result: {other:?}"),
        }
    }

    /// 5xx 脱敏错误在本地化翻译后仍不得泄露 extra / exception。
    #[test]
    fn redacted_500_translation_never_leaks_extra_or_exception() {
        use axum::response::IntoResponse;

        let catalog = Catalog::new("en")
            .add("zh", "E_DB", "数据库服务暂不可用")
            .add("zh", "db", "内部数据库错误");

        // 情况一：带显式 code 的 500 错误，翻译 message 且保持 extra / exception 隐去
        let err = BaseError::new("relation users does not exist")
            .with_category("db")
            .with_sub_category("sqlx")
            .with_code("E_DB")
            .with_status(500)
            .with_exception(true)
            .add_extra("pg=primary");
        let res = localize_response(&catalog, "zh", err.into_response());
        let body = ready_body_json(res);
        assert_eq!(body["message"], "数据库服务暂不可用");
        assert_eq!(body["code"], "E_DB");
        assert!(body.get("extra").is_none());
        assert!(body.get("exception").is_none());

        // 情况二：无显式 code 的 500 错误，保留通用脱敏文案，不被泛化分类覆盖
        let err = BaseError::new("secret")
            .with_category("db")
            .with_status(500)
            .with_exception(true);
        let res = localize_response(&catalog, "zh", err.into_response());
        let body = ready_body_json(res);
        assert_eq!(body["message"], "internal server error");
        assert!(body.get("exception").is_none());
    }

    #[test]
    fn rfc9110_accept_language_qvalue_and_multi_segment_fallback() {
        let catalog = Catalog::new("en")
            .add("zh-hant", "k", "繁體")
            .add("zh", "k", "简体")
            .add("en", "k", "English");
        // 大写 Q=0 显式拒绝 zh-hant，回退到 zh；zh-hant-hk 多级 rsplit_once 回退
        assert_eq!(catalog.negotiate("zh-Hant-HK;Q=0.9, en;q=0.5"), "zh-hant");
        assert_eq!(catalog.negotiate("zh-hant;Q=0, zh;q=0.5, en;q=0.8"), "en");
        // 拒绝项不得通过前缀回退命中
        assert_eq!(catalog.negotiate("zh;q=0, zh-cn;q=0.9, en;q=0.4"), "en");
        // 超过 16 项截断，不触发无界解析
        let many = (0..30)
            .map(|i| format!("x{i};q=0.1"))
            .chain(std::iter::once("zh;q=1".to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(catalog.negotiate(&many), "en");
    }

    #[test]
    fn single_pass_interpolate_and_scoped_error_lookup_with_vary() {
        use axum::response::IntoResponse;

        assert_eq!(
            interpolate(
                "{a} + {b} + {a} + {missing} + {unclosed",
                &[("a", "{b}"), ("b", "2")]
            ),
            "{b} + 2 + {b} + {missing} + {unclosed"
        );

        let catalog = Catalog::new("en")
            .add("en", "session.not_logged_in", "Please sign in.")
            .add("zh", "session.not_logged_in", "请先登录。")
            .add("zh", "session", "会话错误。")
            .add("zh", "fallback_sub", "按 sub_category 命中。");

        let err = BaseError::new("user not login")
            .with_category("session")
            .with_sub_category("not_logged_in")
            .with_status(401);
        let mut res = err.into_response();
        res.headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Origin"));
        let res = localize_response(&catalog, "zh-CN", res);
        let vary = res
            .headers()
            .get(header::VARY)
            .and_then(|v| v.to_str().ok())
            .unwrap();
        assert!(
            vary.contains("Origin") && vary.contains("Accept-Language"),
            "{vary}"
        );
        let body = ready_body_json(res);
        assert_eq!(body["message"], "请先登录。");

        // 无 sub_category 精确 scoped 项时，先试 bare sub_category，再回退 category
        let err_cat = BaseError::new("other")
            .with_category("session")
            .with_sub_category("unknown_sub")
            .with_status(400);
        let res_cat = localize_response(&catalog, "zh", err_cat.into_response());
        let body_cat = ready_body_json(res_cat);
        assert_eq!(body_cat["message"], "会话错误。");
    }
}
