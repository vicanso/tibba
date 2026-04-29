// Copyright 2025 Tree xie.
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

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:session=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:session";

mod middleware;
mod session;

pub use middleware::*;
pub use session::*;

#[derive(Debug, Snafu)]
pub enum Error {
    /// Session ID 为空，通常表示尚未登录或 Session 已重置。
    #[snafu(display("session id is empty"))]
    SessionIdEmpty,
    /// Session ID 格式非法（长度不足 36 字符）。
    #[snafu(display("session id is invalid"))]
    SessionIdInvalid,
    /// Session 缓存未初始化，属于服务端配置错误。
    #[snafu(display("session cache is not set"))]
    SessionCacheNotSet,
    /// Cookie 签名密钥错误。
    #[snafu(display("{source}"))]
    Key { source: cookie::KeyError },
    /// 请求扩展中未找到 Session，通常表示 session 中间件未挂载。
    #[snafu(display("session not found"))]
    SessionNotFound,
    /// 用户未登录，HTTP 401。
    #[snafu(display("user not login"))]
    UserNotLogin,
    /// 用户无管理员权限，HTTP 403。
    #[snafu(display("user not admin"))]
    UserNotAdmin,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            // 服务端内部异常，返回 500
            e @ (Error::SessionIdEmpty
            | Error::SessionIdInvalid
            | Error::SessionCacheNotSet
            | Error::SessionNotFound) => BaseError::new(e.to_string())
                .with_status(500)
                .with_exception(true),

            // Cookie 密钥错误，视为服务端异常
            Error::Key { source } => BaseError::new(source)
                .with_sub_category("cookie")
                .with_status(500)
                .with_exception(true),

            // 未登录，返回 401
            Error::UserNotLogin => BaseError::new("user not login")
                .with_sub_category("user")
                .with_status(401)
                .with_exception(false),

            // 无权限，返回 403
            Error::UserNotAdmin => BaseError::new("user not admin")
                .with_sub_category("user")
                .with_status(403)
                .with_exception(false),
        };
        err.with_category("session")
    }
}
