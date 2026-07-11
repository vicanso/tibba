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

//! 中间件栈开关：描述启动时应挂载哪些横切能力。
//!
//! 各中间件状态类型不同，无法用单一泛型 `ServiceBuilder` 完全抽象；本结构体作为
//! **配置面**，由应用入口按开关组装 layer。默认全开，脚手架/最小服务可关掉不需要的项。

/// 可选中间件开关（默认与当前主应用行为一致：全开）。
#[derive(Debug, Clone)]
pub struct MiddlewareOptions {
    /// OpenTelemetry 入站 span（无 OTLP endpoint 时本就 no-op，仍可关以省一层）
    pub otel: bool,
    /// GET ETag / If-None-Match → 304
    pub http_cache: bool,
    /// Accept-Language 错误消息本地化
    pub i18n: bool,
    /// API Key（`X-API-Key` / `Authorization: Bearer tibba_…`）注入 Session
    pub api_key: bool,
    /// Double-submit CSRF（状态变更 + Cookie 会话）
    pub csrf: bool,
}

impl Default for MiddlewareOptions {
    fn default() -> Self {
        Self {
            otel: true,
            http_cache: true,
            i18n: true,
            api_key: true,
            csrf: true,
        }
    }
}

impl MiddlewareOptions {
    /// 全部关闭的可选层（仅保留 entry/stats/session/security 等硬依赖由入口自管）。
    #[must_use]
    pub fn minimal() -> Self {
        Self {
            otel: false,
            http_cache: false,
            i18n: false,
            api_key: false,
            csrf: false,
        }
    }

    #[must_use]
    pub fn with_otel(mut self, on: bool) -> Self {
        self.otel = on;
        self
    }

    #[must_use]
    pub fn with_http_cache(mut self, on: bool) -> Self {
        self.http_cache = on;
        self
    }

    #[must_use]
    pub fn with_i18n(mut self, on: bool) -> Self {
        self.i18n = on;
        self
    }

    #[must_use]
    pub fn with_api_key(mut self, on: bool) -> Self {
        self.api_key = on;
        self
    }

    #[must_use]
    pub fn with_csrf(mut self, on: bool) -> Self {
        self.csrf = on;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_on() {
        let o = MiddlewareOptions::default();
        assert!(o.otel && o.http_cache && o.i18n && o.api_key && o.csrf);
    }

    #[test]
    fn minimal_is_all_off() {
        let o = MiddlewareOptions::minimal();
        assert!(!o.otel && !o.http_cache && !o.i18n && !o.api_key && !o.csrf);
    }

    #[test]
    fn fluent_toggles() {
        let o = MiddlewareOptions::minimal()
            .with_csrf(true)
            .with_api_key(true);
        assert!(o.csrf && o.api_key);
        assert!(!o.otel);
    }
}
