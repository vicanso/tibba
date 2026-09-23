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
use tibba_error::Error as BaseError;

mod app_config;

/// 配置模块的错误类型。
///
/// 三类来源分别对应：构建 Config（Build）、读取/反序列化配置项（Read）、
/// 解析人类可读字节大小（ParseSize）。所有变体经 `From<Error> for BaseError`
/// 统一带上 `category = "config"`，并标记为异常级（启动期错误应当告警）。
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("build config: {source}"))]
    Build { source: config::ConfigError },
    #[snafu(display("read config: {source}"))]
    Read { source: config::ConfigError },
    #[snafu(display("parse size: {source}"))]
    ParseSize { source: parse_size::Error },
    /// 时长配置既不是 humantime 格式（`10s` / `1h`）也不是纯数字秒数。
    /// 带上键名与实际取值，避免运维只看到一句笼统的类型错误。
    #[snafu(display("invalid duration at {key}: {value:?}"))]
    InvalidDuration { key: String, value: String },

    /// 字节大小超出本平台 `usize` 能表示的范围（32 位目标上的 `8GB` 等）。
    #[snafu(display("byte size at {key} does not fit in usize: {value}"))]
    InvalidByteSize { key: String, value: u64 },

    /// `*_FILE` 指向的密钥文件读不出来。
    ///
    /// **必须 fail fast**：读不到密钥就退回 TOML 里的占位值，等于带着一个
    /// 人畜无害的默认口令跑起来——这比起不来危险得多。
    #[snafu(display("read secret file for {key} at {path}: {source}"))]
    SecretFile {
        key: String,
        path: String,
        source: std::io::Error,
    },

    /// `*_FILE` 指向的文件内容为空。
    ///
    /// 空密钥一定是部署失误（secret 没挂上、挂错了路径）。若放行，
    /// `ignore_empty` 会把它当作「未设置」，于是静默回落到 TOML 的默认值。
    #[snafu(display("secret file for {key} at {path} is empty"))]
    EmptySecretFile { key: String, path: String },

    /// 同一项同时给了 `X` 与 `X_FILE`，无法判断该用哪个。
    #[snafu(display("both {key} and {file_key} are set; they are exclusive"))]
    ConflictingSecret { key: String, file_key: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            // 构建失败发生在启动期，单独打 sub_category 便于日志定位
            Error::Build { source } => BaseError::new(source).with_sub_category("build"),
            // 运行期读取错误占绝大多数，沿用外层 category 即可，不再赘加 sub
            Error::Read { source } => BaseError::new(source),
            Error::ParseSize { source } => BaseError::new(source).with_sub_category("parse_size"),
            Error::InvalidDuration { key, value } => {
                BaseError::new(format!("invalid duration at {key}: {value:?}"))
                    .with_sub_category("invalid_duration")
            }
            Error::InvalidByteSize { key, value } => {
                BaseError::new(format!("byte size at {key} does not fit in usize: {value}"))
                    .with_sub_category("invalid_byte_size")
            }
            // 三者都是启动期的部署配置错误。注意只带 key / path，
            // 绝不把读出来的内容放进错误信息——那正是要保护的密钥
            Error::SecretFile { key, path, source } => {
                BaseError::new(format!("read secret file for {key} at {path}: {source}"))
                    .with_sub_category("secret_file")
            }
            Error::EmptySecretFile { key, path } => {
                BaseError::new(format!("secret file for {key} at {path} is empty"))
                    .with_sub_category("empty_secret_file")
            }
            Error::ConflictingSecret { key, file_key } => BaseError::new(format!(
                "both {key} and {file_key} are set; they are exclusive"
            ))
            .with_sub_category("conflicting_secret"),
        };
        err.with_category("config").with_exception(true)
    }
}

pub use app_config::*;
pub use bytesize_serde;
pub use humantime_serde;
