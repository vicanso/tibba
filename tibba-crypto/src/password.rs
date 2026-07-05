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

//! 密码哈希：Argon2id（服务端加盐、慢哈希，抵抗离线爆破与 pass-the-hash）。
//!
//! 存储格式为 PHC 字符串（`$argon2id$v=19$m=...,t=...,p=...$<salt>$<hash>`），
//! 自带算法参数与盐，校验时无需外部配置。
//!
//! ## 为何加盐哈希
//! 此前版本把客户端 `sha256(password)`（64 位十六进制）**明文存库**：该值本身即是
//! 登录挑战所需的全部材料，任何一次库泄漏（备份 / 副本 / 注入 / 内鬼）即可直接过密码
//! 校验（pass-the-hash），无需破解。改用 Argon2id 后，库中只有单向哈希，攻击者拿到也
//! 必须逐账号高成本爆破。
//!
//! ## 兼容旧数据（无感升级）
//! [`verify_password`] 识别历史遗留的明文 sha256 值并按常数时间比对，命中时返回
//! [`PasswordCheck::MatchedNeedsRehash`]，调用方应借这次成功登录用 Argon2 重写该用户
//! 的密码列，使旧值自然消亡。

use crate::{Argon2HashSnafu, Argon2ParseSnafu, Error};
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand_core::OsRng;
use snafu::ResultExt;

type Result<T, E = Error> = std::result::Result<T, E>;

/// 密码校验结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordCheck {
    /// 校验通过，存储已是 Argon2 哈希，无需额外处理。
    Matched,
    /// 校验通过，但存储的是旧式明文 sha256——调用方应重新哈希写回（无感升级）。
    MatchedNeedsRehash,
    /// 校验失败（密码不匹配）。
    Mismatch,
}

/// 用 Argon2id（默认参数）对 `secret` 加盐哈希，返回可直接入库的 PHC 字符串。
///
/// `secret` 通常是客户端已 `sha256(password)` 处理的定长凭证——再套一层 Argon2 以获得
/// 加盐、慢哈希与 pass-the-hash 抵抗；即便直接传原始口令也同样适用。
pub fn hash_password(secret: &[u8]) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(secret, &salt)
        .context(Argon2HashSnafu)?;
    Ok(hash.to_string())
}

/// 校验 `secret` 是否匹配已存储的哈希 `stored`。
///
/// - `stored` 为 Argon2 PHC 串：走标准 Argon2 验证（内部常数时间）。
/// - `stored` 为 64 位十六进制（旧式明文 sha256）：常数时间比对，命中返回
///   [`PasswordCheck::MatchedNeedsRehash`] 提示调用方升级。
pub fn verify_password(stored: &str, secret: &[u8]) -> Result<PasswordCheck> {
    // 旧式明文 sha256（无 `$` 前缀、恰好 64 位 hex）——常数时间比对，命中提示升级
    if is_legacy_sha256(stored) {
        return if constant_time_eq(stored.as_bytes(), secret) {
            Ok(PasswordCheck::MatchedNeedsRehash)
        } else {
            Ok(PasswordCheck::Mismatch)
        };
    }

    let parsed = PasswordHash::new(stored).context(Argon2ParseSnafu)?;
    match Argon2::default().verify_password(secret, &parsed) {
        Ok(()) => Ok(PasswordCheck::Matched),
        // 仅「密码不匹配」归为 Mismatch；其余（哈希损坏 / 参数异常）作为服务端错误上抛
        Err(argon2::password_hash::Error::Password) => Ok(PasswordCheck::Mismatch),
        Err(source) => Err(Error::Argon2Parse { source }),
    }
}

/// 判断 `stored` 是否为旧式明文 sha256：恰好 64 位、全部十六进制字符。
/// Argon2 PHC 串以 `$argon2` 开头，天然不满足此条件。
fn is_legacy_sha256(stored: &str) -> bool {
    stored.len() == 64 && stored.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 常数时间比较，避免按字节短路造成的时序泄漏。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_roundtrip() {
        let secret = b"a3f1c9deadbeef";
        let stored = hash_password(secret).unwrap();
        // PHC 串以 $argon2 开头，且不会被误判为旧式 sha256
        assert!(stored.starts_with("$argon2"));
        assert!(!is_legacy_sha256(&stored));
        assert_eq!(
            verify_password(&stored, secret).unwrap(),
            PasswordCheck::Matched
        );
        assert_eq!(
            verify_password(&stored, b"wrong").unwrap(),
            PasswordCheck::Mismatch
        );
    }

    #[test]
    fn legacy_sha256_matches_and_flags_rehash() {
        // 模拟旧库：64 位 hex 明文
        let legacy = "e".repeat(64);
        assert!(is_legacy_sha256(&legacy));
        assert_eq!(
            verify_password(&legacy, legacy.as_bytes()).unwrap(),
            PasswordCheck::MatchedNeedsRehash
        );
        assert_eq!(
            verify_password(&legacy, "f".repeat(64).as_bytes()).unwrap(),
            PasswordCheck::Mismatch
        );
    }

    #[test]
    fn distinct_salts_produce_distinct_hashes() {
        let secret = b"same-input";
        assert_ne!(
            hash_password(secret).unwrap(),
            hash_password(secret).unwrap()
        );
    }
}
