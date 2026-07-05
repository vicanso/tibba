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

#[derive(Debug, Snafu)]
pub enum Error {
    /// 封装 hmac crate 的 `InvalidLength`，使调用方可直接使用 `.context(HmacSha256Snafu)`。
    #[snafu(display("hmac sha256 error: {source}"))]
    HmacSha256 { source: hmac::digest::InvalidLength },

    /// 密钥列表为空，无法执行签名或验签操作。
    #[snafu(display("key grip empty"))]
    KeyGripEmpty,

    /// Argon2 哈希计算失败（参数异常，正常不会发生）。
    #[snafu(display("argon2 hash error: {source}"))]
    Argon2Hash {
        source: argon2::password_hash::Error,
    },

    /// 解析已存储的 Argon2 PHC 串失败（库中哈希损坏 / 校验阶段内部异常）。
    #[snafu(display("argon2 parse error: {source}"))]
    Argon2Parse {
        source: argon2::password_hash::Error,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::HmacSha256 { source } => BaseError::new(source).with_sub_category("hmac_sha256"),
            Error::KeyGripEmpty => BaseError::new("key grip empty")
                .with_sub_category("key_grip")
                .with_status(500)
                .with_exception(true),
            Error::Argon2Hash { source } => BaseError::new(source)
                .with_sub_category("argon2_hash")
                .with_status(500)
                .with_exception(true),
            Error::Argon2Parse { source } => BaseError::new(source)
                .with_sub_category("argon2_parse")
                .with_status(500)
                .with_exception(true),
        };
        err.with_category("crypto")
    }
}

mod key_grip;
mod password;

pub use key_grip::*;
pub use password::*;
