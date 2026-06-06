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
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            // 构建失败发生在启动期，单独打 sub_category 便于日志定位
            Error::Build { source } => BaseError::new(source).with_sub_category("build"),
            // 运行期读取错误占绝大多数，沿用外层 category 即可，不再赘加 sub
            Error::Read { source } => BaseError::new(source),
            Error::ParseSize { source } => BaseError::new(source).with_sub_category("parse_size"),
        };
        err.with_category("config").with_exception(true)
    }
}

pub use app_config::*;
pub use bytesize_serde;
pub use humantime_serde;
