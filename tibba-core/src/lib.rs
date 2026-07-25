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

//! tibba 核心组件的门面（facade）。
//!
//! 本 crate **不含任何业务代码**，只把 7 个 core crate 收拢到一个依赖项下，
//! 让下游从写 7 行依赖、对齐 7 个版本号变成写一行：
//!
//! ```toml
//! tibba-core = "0.2.6"
//! ```
//!
//! ```ignore
//! use tibba_core::error::Error;
//! use tibba_core::runtime::AppState;
//! use tibba_core::cache::RedisCache;
//! ```
//!
//! # 模块与 feature
//!
//! | 模块 | 对应 crate | feature | 独占的重依赖 |
//! |------|-----------|---------|-------------|
//! | [`error`] | `tibba-error` | 始终编译 | axum |
//! | `util` | `tibba-util` | `util` | chrono / uuid / zstd / validator |
//! | `config` | `tibba-config` | `config` | config-rs |
//! | `crypto` | `tibba-crypto` | `crypto` | argon2 / hmac / sha2 |
//! | `runtime` | `tibba-runtime` | `runtime` | arc-swap / dashmap |
//! | `cache` | `tibba-cache` | `cache` | redis / deadpool |
//! | `request` | `tibba-request` | `request` | reqwest / opentelemetry |
//!
//! 另有两个透传 feature：`process-info`（`runtime` 的 sysinfo 进程采样）与
//! `scheduler`（`runtime` 的 cron 定时任务）。
//!
//! `default = ["full"]` 全部打开——门面的价值就是「写一行就能用」。需要精简时：
//!
//! ```toml
//! # 只要错误类型：55 个传递依赖，而 full 是 269 个
//! tibba-core = { version = "0.2.6", default-features = false }
//! # 按需组合
//! tibba-core = { version = "0.2.6", default-features = false, features = ["config", "cache"] }
//! ```
//!
//! # 什么时候**不**该用本 crate
//!
//! 库 crate（尤其是 workspace 内的 `tibba-*`）应当继续直接依赖它真正需要的那几个
//! core crate，而不是经由门面。原因是 7 个 crate 的边界本身有价值：
//!
//! - **依赖可裁剪**：`tibba-sql` 只用到 `tibba-util::parse_uri`，不该背上 redis 与 reqwest。
//! - **增量编译粒度**：改 `tibba-cache` 一行只重编 cache 及其下游，不会波及全部 core。
//! - **分层由编译器强制**：跨层引用会直接编译失败，而不是靠自觉。
//!
//! 门面是给**最终应用**用的——那里本来就要用到全部 core，收拢依赖是净收益。

/// HTTP 错误类型（`tibba-error`）。始终可用。
pub use tibba_error as error;

/// Redis 缓存（`tibba-cache`）。需 feature `cache`。
#[cfg(feature = "cache")]
pub use tibba_cache as cache;

/// 配置加载（`tibba-config`）。需 feature `config`。
#[cfg(feature = "config")]
pub use tibba_config as config;

/// 密码哈希与密钥（`tibba-crypto`）。需 feature `crypto`。
#[cfg(feature = "crypto")]
pub use tibba_crypto as crypto;

/// 出站 HTTP 客户端（`tibba-request`）。需 feature `request`。
#[cfg(feature = "request")]
pub use tibba_request as request;

/// 进程运行时状态、指标与生命周期（`tibba-runtime`）。需 feature `runtime`。
#[cfg(feature = "runtime")]
pub use tibba_runtime as runtime;

/// 通用工具与自定义校验器（`tibba-util`）。需 feature `util`。
#[cfg(feature = "util")]
pub use tibba_util as util;

#[cfg(test)]
mod tests {
    // 门面必须保持纯 re-export，不能退化成 wrapper 模块。
    // 若有人把 `pub use tibba_error as error;` 改成自定义包装，下游同时依赖
    // tibba-core 与 tibba-error 时会拿到两套互不兼容的类型；下面的赋值/传参
    // 在那种情况下会编译失败，从而在 CI 上先暴露出来。

    #[test]
    fn error_module_is_pure_reexport() {
        fn takes_base(_: tibba_error::Error) {}
        takes_base(crate::error::Error::new("probe"));
    }

    #[cfg(feature = "runtime")]
    #[test]
    fn runtime_module_is_pure_reexport() {
        fn takes_base(_: &tibba_runtime::AppState) {}
        let state = crate::runtime::AppState::new(-1, "probe");
        takes_base(&state);
    }
}
