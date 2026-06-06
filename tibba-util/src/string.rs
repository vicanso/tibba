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

use super::timestamp;
use hex::encode;
use nanoid::nanoid;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use tibba_error::Error;
use uuid::{NoContext, Timestamp, Uuid};

type Result<T> = std::result::Result<T, Error>;

const SIGNATURE_TTL_SECS: i64 = 5 * 60; // 5 minutes

/// 常数时间字节切片比较，避免哈希校验时按字节短路泄露时序侧信道。
///
/// 注意：长度不一致直接早返回 false——签名长度固定（SHA-256 hex 始终 64 字符），
/// 长度本身是公开信息，不构成额外侧信道。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // XOR 累加：所有字节相等 ⇔ acc == 0；不论结果如何，循环总扫完整个切片
    let acc = a
        .iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y));
    acc == 0
}

/// Generates a UUIDv7 string
///
/// Creates a time-based UUID (version 7) using the current system time
/// Format: xxxxxxxx-xxxx-7xxx-xxxx-xxxxxxxxxxxx
///
/// # Returns
/// * String containing the formatted UUID
///
/// # Note
/// UUIDv7 provides:
/// - Timestamp-based ordering
/// - Monotonic ordering within the same timestamp
/// - Standards compliance
pub fn uuid() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ts = Timestamp::from_unix(NoContext, d.as_secs(), d.subsec_nanos());
    Uuid::new_v7(ts).to_string()
}

/// Generates a NanoID string of specified length
///
/// Creates a URL-safe, unique string using NanoID algorithm
///
/// # Arguments
/// * `size` - Length of the generated ID
///
/// # Returns
/// * String containing the NanoID
///
/// # Note
/// NanoID provides:
/// - URL-safe characters
/// - Configurable length
/// - High collision resistance
pub fn nanoid(size: usize) -> String {
    nanoid!(size)
}

/// Formats a floating-point number with specified precision
///
/// Converts float to string with fixed number of decimal places
/// Supports precision from 0 to 4 decimal places
///
/// # Arguments
/// * `value` - Floating point number to format
/// * `precision` - Number of decimal places (0-4)
///
/// # Returns
/// * String containing formatted number
///
/// # Examples
/// ```ignore
/// assert_eq!("1.12", float_to_fixed(1.123412, 2));
/// assert_eq!("1", float_to_fixed(1.123412, 0));
/// ```
pub fn float_to_fixed(value: f64, precision: usize) -> String {
    let p = precision.min(4);
    format!("{value:.p$}")
}

fn sha256_multi(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    encode(hasher.finalize())
}

/// Computes the SHA-256 hash of the input data
///
/// # Arguments
/// * `data` - Input data to hash
///
/// # Returns
/// * String containing the SHA-256 hash
pub fn sha256(data: &[u8]) -> String {
    sha256_multi(&[data])
}

/// Computes the SHA-256 hash of the input data and signs it with the secret key
///
/// # Arguments
/// * `value` - Input data to hash
/// * `secret` - Secret key
///
/// # Returns
/// * String containing the signed hash
pub fn sign_hash(value: &str, secret: &str) -> String {
    sha256_multi(&[value.as_bytes(), b":", secret.as_bytes()])
}

/// Computes the SHA-256 hash of the input data and signs it with the secret key
///
/// # Arguments
/// * `value` - Input data to hash
/// * `secret` - Secret key
///
/// # Returns
/// * Tuple containing the timestamp and the signed hash
pub fn timestamp_hash(value: &str, secret: &str) -> (i64, String) {
    let ts = timestamp();
    let ts_str = ts.to_string();
    let hash = sha256_multi(&[
        ts_str.as_bytes(),
        b":",
        value.as_bytes(),
        b":",
        secret.as_bytes(),
    ]);
    (ts, hash)
}

/// Validates the signature of the input data
///
/// # Arguments
/// * `value` - Input data to hash
/// * `hash` - Signature to validate
/// * `secret` - Secret key
///
/// # Returns
/// * Result containing the validation result
pub fn validate_sign_hash(value: &str, hash: &str, secret: &str) -> Result<()> {
    // 走常数时间比较：直接 `!=` 比较 hex 字符串会泄露逐字节比较的时序，
    // 攻击者可借此推断签名前缀字节（同 tibba-crypto/key_grip 的 verify_slice 修复）
    let expected = sign_hash(value, secret);
    if !constant_time_eq(expected.as_bytes(), hash.as_bytes()) {
        return Err(Error::new("signature is invalid").with_category("sign_hash"));
    }
    Ok(())
}

/// Validates the signature of the input data
///
/// # Arguments
/// * `ts` - Timestamp
/// * `value` - Input data to hash
/// * `hash` - Signature to validate
/// * `secret` - Secret key
///
/// # Returns
/// * Result containing the validation result
pub fn validate_timestamp_hash(ts: i64, value: &str, hash: &str, secret: &str) -> Result<()> {
    let category = "timestamp_hash";
    if (timestamp() - ts).abs() > SIGNATURE_TTL_SECS {
        return Err(Error::new("signature is expired").with_category(category));
    }
    let ts_str = ts.to_string();
    let expected_hash = sha256_multi(&[
        ts_str.as_bytes(),
        b":",
        value.as_bytes(),
        b":",
        secret.as_bytes(),
    ]);

    // 走常数时间比较，避免按字节短路泄露时序（同 validate_sign_hash 的处理）
    if !constant_time_eq(expected_hash.as_bytes(), hash.as_bytes()) {
        return Err(Error::new("signature is invalid").with_category(category));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Tests float_to_fixed function with various precisions
    #[test]
    fn to_fixed() {
        assert_eq!("1", float_to_fixed(1.123412, 0));
        assert_eq!("1.1", float_to_fixed(1.123412, 1));
        assert_eq!("1.12", float_to_fixed(1.123412, 2));
        assert_eq!("1.123", float_to_fixed(1.123412, 3));
        assert_eq!("1.1234", float_to_fixed(1.123412, 4));
        // precision >4 被截断为 4，避免过长格式串带来的开销
        assert_eq!("1.1234", float_to_fixed(1.123412, 10));
    }

    #[test]
    fn sha256_is_stable_lowercase_hex() {
        // SHA-256("hello") 的标准向量；同时验证大小写与长度
        let h = sha256(b"hello");
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn sign_hash_round_trip() {
        let sig = sign_hash("payload", "secret");
        assert!(validate_sign_hash("payload", &sig, "secret").is_ok());
        // 任一字段变化都应当让校验失败
        assert!(validate_sign_hash("payloadX", &sig, "secret").is_err());
        assert!(validate_sign_hash("payload", &sig, "secretX").is_err());
        assert!(validate_sign_hash("payload", "deadbeef", "secret").is_err());
    }

    #[test]
    fn timestamp_hash_round_trip() {
        let (ts, sig) = timestamp_hash("payload", "secret");
        assert!(validate_timestamp_hash(ts, "payload", &sig, "secret").is_ok());
        // 过期时间戳应当被拒绝
        let expired_ts = ts - (SIGNATURE_TTL_SECS + 1);
        let expired_sig = sha256_multi(&[
            expired_ts.to_string().as_bytes(),
            b":",
            b"payload",
            b":",
            b"secret",
        ]);
        let err =
            validate_timestamp_hash(expired_ts, "payload", &expired_sig, "secret").unwrap_err();
        assert!(err.to_string().contains("expired"));
        // 篡改 payload 也应失败
        assert!(validate_timestamp_hash(ts, "payloadX", &sig, "secret").is_err());
    }

    #[test]
    fn constant_time_eq_basic() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        // 长度不同直接 false（长度本身公开，可早返回）
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"x"));
        // 空切片相等
        assert!(constant_time_eq(b"", b""));
    }
}
