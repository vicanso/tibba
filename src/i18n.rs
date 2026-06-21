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

//! 应用级 i18n 目录：登记常见错误 key 的中英文译文，启动期注册到 [`tibba_i18n`]。
//!
//! key 取错误的 `code` / `sub_category` / `category`（见 `tibba_i18n` 中间件）。这里登记的
//! 都是既有错误已带的 `sub_category` / `category`，因此无需改动任何错误构造处即可本地化。
//! 业务新增可本地化错误时，给错误设 `with_sub_category`/`with_code` 并在此登记对应译文即可。

use tibba_i18n::Catalog;

/// 默认（回退）语言。`Accept-Language` 协商不到受支持语言时返回英文。
const FALLBACK: &str = "en";

/// 构建并注册全局本地化目录。重复调用安全（已注册则忽略）。
pub fn init() {
    let catalog = Catalog::new(FALLBACK)
        // 限流 / 并发超限（tibba-middleware）
        .add("en", "too_many_requests", "Too many requests, please retry later.")
        .add("zh", "too_many_requests", "请求过于频繁，请稍后再试。")
        .add("en", "rate_limited", "Rate limit exceeded, please slow down.")
        .add("zh", "rate_limited", "已触发频率限制，请稍后再试。")
        // CSRF 校验失败（tibba-middleware）
        .add("en", "csrf_cookie_missing", "Missing CSRF cookie.")
        .add("zh", "csrf_cookie_missing", "缺少 CSRF cookie。")
        .add("en", "csrf_header_missing", "Missing CSRF header.")
        .add("zh", "csrf_header_missing", "缺少 CSRF 请求头。")
        .add("en", "csrf_mismatch", "CSRF token mismatch.")
        .add("zh", "csrf_mismatch", "CSRF 令牌不匹配，疑似伪造请求。")
        // 请求超时（main::handle_error，category = "timeout"）
        .add("en", "timeout", "Request took too long.")
        .add("zh", "timeout", "请求处理超时，请稍后重试。");
    // 已注册（如热重载重复调用）则忽略，不覆盖
    let _ = tibba_i18n::init(catalog);
}
