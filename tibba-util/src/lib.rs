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

use snafu::Snafu;
use std::env;
use std::sync::LazyLock;
use tibba_error::Error as BaseError;

// Error 变体按 feature 门控：Zstd / Lz4Decompress 只被 compression.rs 构造，
// InvalidHeaderName / InvalidHeaderValue 只被 http.rs 构造，
// 对应 feature 关闭时变体一并消失，避免 enum 携带无法到达的分支。
#[derive(Snafu, Debug)]
pub enum Error {
    #[cfg(feature = "compression")]
    #[snafu(display("{source}"))]
    Zstd { source: std::io::Error },
    #[cfg(feature = "compression")]
    #[snafu(display("{source}"))]
    Lz4Decompress {
        source: lz4_flex::block::DecompressError,
    },
    /// 解压输出超过允许上限，见 `decompress_with_limit`。
    #[cfg(feature = "compression")]
    #[snafu(display("decompressed size {size} exceeds limit {limit}"))]
    DecompressTooLarge { size: usize, limit: usize },
    #[cfg(feature = "http")]
    #[snafu(display("{source}"))]
    InvalidHeaderName {
        source: axum::http::header::InvalidHeaderName,
    },
    #[cfg(feature = "http")]
    #[snafu(display("{source}"))]
    InvalidHeaderValue {
        source: axum::http::header::InvalidHeaderValue,
    },
    #[snafu(display("{message}"))]
    Invalid { message: String },
    #[snafu(display("{source}"))]
    Deserialize { source: serde_urlencoded::de::Error },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        // 单次 match：从 source 构造 BaseError 并打 sub_category，
        // 与项目其他 snafu 模块（tibba-config / tibba-sql 等）保持一致
        let err = match val {
            #[cfg(feature = "compression")]
            Error::Zstd { source } => BaseError::new(source).with_sub_category("zstd"),
            #[cfg(feature = "compression")]
            Error::Lz4Decompress { source } => {
                BaseError::new(source).with_sub_category("lz4_decompress")
            }
            // 超限多半意味着数据损坏或被篡改，值得告警
            #[cfg(feature = "compression")]
            Error::DecompressTooLarge { size, limit } => {
                BaseError::new(format!("decompressed size {size} exceeds limit {limit}"))
                    .with_sub_category("decompress_too_large")
                    .with_exception(true)
            }
            #[cfg(feature = "http")]
            Error::InvalidHeaderName { source } => {
                BaseError::new(source).with_sub_category("invalid_header_name")
            }
            #[cfg(feature = "http")]
            Error::InvalidHeaderValue { source } => {
                BaseError::new(source).with_sub_category("invalid_header_value")
            }
            Error::Invalid { message } => BaseError::new(message).with_sub_category("invalid"),
            Error::Deserialize { source } => {
                BaseError::new(source).with_sub_category("deserialize")
            }
        };
        err.with_category("util")
    }
}

/// 未设置 `RUST_ENV` 时采用的运行环境。
///
/// # 必须是 production（fail closed）
/// 此前默认是 `dev`，而 Dockerfile / entrypoint / 部署脚本都没有设置 `RUST_ENV`——
/// 也就是说**默认构建出来的镜像以开发模式运行**，一口气打开了下面这些口子：
///
/// - 验证码可以用固定的 `1234` 绕过（`magic_code`）
/// - 允许沿用脚手架自带的占位 `basic.secret`，session 签名密钥等于公开
/// - 登录防重放可以用 `ts=0` 整个跳过
/// - session / CSRF / 设备 cookie 不带 `Secure`
/// - Swagger UI 对外暴露，CORS 的生产安全校验被跳过
///
/// 这些放宽都是为本地开发准备的，默认值理应站在最安全的一侧：忘了设环境变量
/// 的后果应该是「本地开发多敲一行 `RUST_ENV=dev`」，而不是「线上门户大开」。
pub const DEFAULT_RUST_ENV: &str = "production";

static RUST_ENV: LazyLock<String> =
    LazyLock::new(|| env::var("RUST_ENV").unwrap_or_else(|_| DEFAULT_RUST_ENV.to_string()));

/// 原始的 `RUST_ENV` 值（未设置时为 [`DEFAULT_RUST_ENV`]）。
///
/// 用于按环境选择配置文件（`<env>.toml`）等需要原值的场合；**判断安全相关
/// 行为请用** [`is_development`] / [`is_test`] / [`is_production`]。
pub fn get_env() -> &'static str {
    &RUST_ENV
}

/// 是否为本地开发环境（`RUST_ENV=dev`，必须精确匹配）。
pub fn is_development() -> bool {
    get_env() == "dev"
}

/// 是否为测试环境（`RUST_ENV=test`，必须精确匹配）。
pub fn is_test() -> bool {
    get_env() == "test"
}

/// 是否按生产环境的安全要求运行：**既不是 dev 也不是 test 就是 production**。
///
/// 此前要求精确等于 `"production"`，于是 `staging`、`prod`、`Production` 这类
/// 值三个判断全为 false——既拿不到开发便利，也躲过了所有 `if is_production()`
/// 的生产校验（例如 CORS 安全断言），落在一个谁都没设计过的中间态里。
/// 现在只有显式声明的 dev / test 才放宽，其余一律按生产对待。
pub fn is_production() -> bool {
    !is_development() && !is_test()
}

#[cfg(feature = "compression")]
mod compression;
mod datetime;
#[cfg(feature = "http")]
mod http;
mod request;
mod response;
mod string;
mod uri;
mod validate;
mod value;

#[cfg(feature = "compression")]
pub use compression::*;
pub use datetime::*;
#[cfg(feature = "http")]
pub use http::*;
pub use request::*;
pub use response::*;
pub use string::*;
pub use uri::*;
pub use validate::*;
pub use value::*;
