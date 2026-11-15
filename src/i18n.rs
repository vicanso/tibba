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

//! 应用级 i18n 目录：登记常见错误 key 的中英文译文，启动期注册到 `tibba_middleware::i18n`。

use tibba_middleware::i18n::Catalog;

/// 默认（回退）语言。`Accept-Language` 协商不到受支持语言时返回英文。
const FALLBACK: &str = "en";

/// 构建并注册全局本地化目录。重复调用安全（已注册则忽略）。
pub fn init() {
    let catalog = Catalog::new(FALLBACK)
        // 限流 / 并发超限（tibba-middleware）
        .add(
            "en",
            "too_many_requests",
            "Too many requests, please retry later.",
        )
        .add("zh", "too_many_requests", "请求过于频繁，请稍后再试。")
        .add(
            "en",
            "rate_limited",
            "Rate limit exceeded, please slow down.",
        )
        .add("zh", "rate_limited", "已触发频率限制，请稍后再试。")
        // CSRF 校验失败（tibba-middleware）
        .add("en", "csrf_cookie_missing", "Missing CSRF cookie.")
        .add("zh", "csrf_cookie_missing", "缺少 CSRF cookie。")
        .add("en", "csrf_header_missing", "Missing CSRF header.")
        .add("zh", "csrf_header_missing", "缺少 CSRF 请求头。")
        .add("en", "csrf_mismatch", "CSRF token mismatch.")
        .add("zh", "csrf_mismatch", "CSRF 令牌不匹配，疑似伪造请求。")
        // 请求超时（tibba_middleware::handle_middleware_error）
        .add("en", "timeout", "Request took too long.")
        .add("zh", "timeout", "请求处理超时，请稍后重试。")
        // 认证与权限（tibba-auth）
        .add(
            "en",
            "not_logged_in",
            "Authentication required, please sign in.",
        )
        .add("zh", "not_logged_in", "请先登录后再操作。")
        .add("en", "not_admin", "Administrator privileges required.")
        .add("zh", "not_admin", "需要管理员权限。")
        .add(
            "en",
            "permission_denied",
            "You do not have permission to perform this action.",
        )
        .add("zh", "permission_denied", "当前账号缺少执行该操作的权限。")
        // 幂等与参数校验（tibba-middleware / tibba-model / tibba-router）
        .add(
            "en",
            "idempotency_invalid_key",
            "Invalid Idempotency-Key header.",
        )
        .add(
            "zh",
            "idempotency_invalid_key",
            "幂等键（Idempotency-Key）格式不合法。",
        )
        .add(
            "en",
            "idempotency_conflict",
            "A request with this Idempotency-Key is already in progress.",
        )
        .add(
            "zh",
            "idempotency_conflict",
            "相同幂等键的请求正在处理中，请勿重复提交。",
        )
        .add("en", "validation", "Invalid request parameters.")
        .add("zh", "validation", "请求参数校验未通过。")
        // 登录与账号安全状态（tibba-router）
        .add("en", "bad_credentials", "Incorrect account or password.")
        .add("zh", "bad_credentials", "账号或密码错误。")
        .add(
            "en",
            "too_many_attempts",
            "Too many failed attempts, please try again later.",
        )
        .add("zh", "too_many_attempts", "尝试次数过多，请稍后再试。")
        .add("en", "account_disabled", "This account has been disabled.")
        .add("zh", "account_disabled", "该账号已被禁用。");
    let _ = tibba_middleware::i18n::init(catalog);
}
