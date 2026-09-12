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

    /// 输入的 secret 超过策略允许的长度上限。
    /// Argon2 是慢哈希，超长输入等于免费的 CPU 消耗放大器。
    #[snafu(display("secret too long: {len} bytes (max {max})"))]
    SecretTooLong { len: usize, max: usize },

    /// Argon2 代价参数非法（如 m_cost 小于 8×p_cost）。
    #[snafu(display("invalid argon2 params: {source}"))]
    InvalidParams { source: argon2::Error },

    /// AES-GCM 加密失败（GCM 加密无业务前置条件，实际几乎不发生）。
    #[snafu(display("encryption failed"))]
    Encrypt,

    /// AES-GCM 解密 / 完整性校验失败：密文被篡改、密钥或 AAD 不匹配、数据损坏。
    ///
    /// 刻意不区分这几种原因——区分开就成了攻击者可用的 oracle。
    #[snafu(display("decryption failed"))]
    Decrypt,

    /// 落库密文的 base64 解码失败。
    #[snafu(display("decode encrypted blob: {source}"))]
    Base64 { source: base64::DecodeError },

    /// 密文长度不足以容纳 nonce 与 tag，数据已损坏。
    #[snafu(display("encrypted blob too short"))]
    BlobTooShort,
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
            // 客户端送来的输入过长属请求错误：400、不告警，且文案可直接回给调用方
            Error::SecretTooLong { len, max } => {
                BaseError::new(format!("secret too long: {len} bytes (max {max})"))
                    .with_sub_category("secret_too_long")
                    .with_status(400)
                    .with_exception(false)
            }
            // 参数非法属部署配置错误，启动期就该被发现
            Error::InvalidParams { source } => BaseError::new(source)
                .with_sub_category("invalid_params")
                .with_status(500)
                .with_exception(true),
            // 加解密失败一律 500 + 告警：要么是密钥配置错了，要么是数据被动过，
            // 两种都不是调用方能自行恢复的，且都需要人来看一眼
            Error::Encrypt => BaseError::new("encryption failed")
                .with_sub_category("encrypt")
                .with_status(500)
                .with_exception(true),
            Error::Decrypt => BaseError::new("decryption failed")
                .with_sub_category("decrypt")
                .with_status(500)
                .with_exception(true),
            Error::Base64 { source } => BaseError::new(source)
                .with_sub_category("base64")
                .with_status(500)
                .with_exception(true),
            Error::BlobTooShort => BaseError::new("encrypted blob too short")
                .with_sub_category("blob_too_short")
                .with_status(500)
                .with_exception(true),
        };
        err.with_category("crypto")
    }
}

mod cipher;
mod key_grip;
mod password;

pub use cipher::*;
pub use key_grip::*;
pub use password::*;
