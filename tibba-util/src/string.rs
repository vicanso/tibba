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
use hmac::{Hmac, KeyInit, Mac};
use nanoid::nanoid;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tibba_error::Error;
use uuid::{NoContext, Timestamp, Uuid};

type Result<T> = std::result::Result<T, Error>;

/// HMAC-SHA256 类型别名，与 `tibba-crypto::KeyGrip` 保持一致。
type HmacSha256 = Hmac<Sha256>;

const SIGNATURE_TTL_SECS: i64 = 5 * 60; // 5 minutes

/// 常数时间字节切片比较，避免签名校验时按字节短路泄露时序侧信道。
///
/// 走 `subtle`（workspace 内 `tibba-totp` 已在用）而不是手写 XOR 折叠：手写版本
/// 依赖「编译器不会把这个循环优化成短路比较」这一无法在源码层保证的假设，
/// `subtle` 用优化屏障把它落实下来。长度不一致直接 false——签名长度固定
/// （hex 恒为 64 字符），长度本身是公开信息，不构成额外侧信道。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// 以 `secret` 为密钥对各段做 HMAC-SHA256，返回小写十六进制摘要。
///
/// # 为什么是 HMAC 而不是 `sha256(value ":" secret)`
/// 此前用的是 secret-suffix 构造（把密钥拼在消息后面再整体哈希）。它虽然躲开了
/// secret-prefix 的长度扩展攻击，却是一个**未经标准化审视**的自制 MAC：其安全性
/// 直接押在底层哈希的抗碰撞性上，而 HMAC 的安全性只需要压缩函数是伪随机的，
/// 论证强度完全不同。既然 `hmac` 已经是 workspace 依赖（`tibba-crypto::KeyGrip`
/// 在用），没有理由在另一处自造一个。
///
/// 输出仍是 64 字符十六进制，与旧实现同形，`x_sha256` 这类格式校验无需改动。
fn hmac_sha256(secret: &[u8], parts: &[&[u8]]) -> Result<String> {
    // `Hmac` 接受任意长度密钥（超出块长时先哈希），这里的 Err 实际不可达；
    // 但仍如实上抛而非 unwrap——crate 内禁止 unwrap，且真出错时静默产生一个
    // 错误签名远比报错更难排查。
    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|e| Error::new(e).with_category("sign_hash"))?;
    for part in parts {
        mac.update(part);
    }
    Ok(encode(mac.finalize().into_bytes()))
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

/// 用 `secret` 对 `value` 做 HMAC-SHA256 签名，返回 64 字符十六进制摘要。
///
/// 校验用 [`validate_sign_hash`]。需要带时效的签名用 [`timestamp_hash`]。
///
/// # 兼容性
/// 返回值由 secret-suffix SHA-256 改为 HMAC-SHA256，**同一输入的签名与旧版本不同**。
/// 格式（64 位小写十六进制）未变。
pub fn sign_hash(value: &str, secret: &str) -> Result<String> {
    hmac_sha256(secret.as_bytes(), &[value.as_bytes()])
}

/// 用 `secret` 对「当前时间戳 + `value`」做 HMAC-SHA256 签名。
///
/// 返回 `(ts, hash)`，两者都要回给客户端并在后续请求中原样带回，由
/// [`validate_timestamp_hash`] 校验时效与签名。
///
/// 时间戳**参与签名**，因此客户端无法在不失效的前提下篡改它来延长有效期。
///
/// # 兼容性
/// 同 [`sign_hash`]：算法已换成 HMAC-SHA256，旧签名不再通过校验。签名有效期只有
/// [`SIGNATURE_TTL_SECS`]（5 分钟），滚动发布期间最多出现这一窗口的校验失败，
/// 客户端重新取一次令牌即可。
pub fn timestamp_hash(value: &str, secret: &str) -> Result<(i64, String)> {
    let ts = timestamp();
    let ts_str = ts.to_string();
    let hash = hmac_sha256(secret.as_bytes(), &[ts_str.as_bytes(), b":", value.as_bytes()])?;
    Ok((ts, hash))
}

/// 校验 [`sign_hash`] 产生的签名，不匹配时返回错误。
///
/// 比较走常数时间：直接 `!=` 比 hex 字符串会泄露逐字节比较的时序，
/// 攻击者可借此逐字节推断出合法签名。
pub fn validate_sign_hash(value: &str, hash: &str, secret: &str) -> Result<()> {
    let expected = sign_hash(value, secret)?;
    if !constant_time_eq(expected.as_bytes(), hash.as_bytes()) {
        return Err(Error::new("signature is invalid")
            .with_category("sign_hash")
            .with_status(401));
    }
    Ok(())
}

/// 校验 [`timestamp_hash`] 产生的签名：先查时效，再常数时间比对摘要。
///
/// `ts` 与当前时间相差超过 [`SIGNATURE_TTL_SECS`] 即判为过期。用绝对值比较，
/// 未来时间戳同样受限，避免客户端把 `ts` 调到远期换取一个长期有效的签名。
pub fn validate_timestamp_hash(ts: i64, value: &str, hash: &str, secret: &str) -> Result<()> {
    let category = "timestamp_hash";
    if timestamp().saturating_sub(ts).saturating_abs() > SIGNATURE_TTL_SECS {
        return Err(Error::new("signature is expired")
            .with_category(category)
            .with_status(401));
    }
    let ts_str = ts.to_string();
    let expected_hash =
        hmac_sha256(secret.as_bytes(), &[ts_str.as_bytes(), b":", value.as_bytes()])?;

    if !constant_time_eq(expected_hash.as_bytes(), hash.as_bytes()) {
        return Err(Error::new("signature is invalid")
            .with_category(category)
            .with_status(401));
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
        let sig = sign_hash("payload", "secret").unwrap();
        // 与旧实现同形：64 位小写十六进制，x_sha256 之类的格式校验无需改动
        assert_eq!(sig.len(), 64);
        assert!(sig.bytes().all(|b| b.is_ascii_hexdigit()));

        assert!(validate_sign_hash("payload", &sig, "secret").is_ok());
        // 任一字段变化都应当让校验失败
        assert!(validate_sign_hash("payloadX", &sig, "secret").is_err());
        assert!(validate_sign_hash("payload", &sig, "secretX").is_err());
        assert!(validate_sign_hash("payload", "deadbeef", "secret").is_err());
    }

    /// 签名必须真的依赖密钥：换密钥要得到不同结果，且不能等于无密钥的裸哈希。
    #[test]
    fn sign_hash_is_keyed() {
        let a = sign_hash("payload", "secret-a").unwrap();
        let b = sign_hash("payload", "secret-b").unwrap();
        assert_ne!(a, b);
        assert_ne!(a, sha256(b"payload"), "签名不得退化成无密钥哈希");
    }

    /// **回归守卫**：HMAC 与旧的 secret-suffix 构造必须产出不同摘要。
    /// 若有人把实现改回 `sha256(value ":" secret)`，本例会失败。
    #[test]
    fn sign_hash_is_not_secret_suffix_sha256() {
        let legacy = sha256_multi(&[b"payload", b":", b"secret"]);
        assert_ne!(sign_hash("payload", "secret").unwrap(), legacy);
    }

    #[test]
    fn timestamp_hash_round_trip() {
        let (ts, sig) = timestamp_hash("payload", "secret").unwrap();
        assert!(validate_timestamp_hash(ts, "payload", &sig, "secret").is_ok());

        // 过期时间戳应当被拒绝（签名本身是对的，只是超出有效期）
        let expired_ts = ts - (SIGNATURE_TTL_SECS + 1);
        let expired_sig = hmac_sha256(
            b"secret",
            &[expired_ts.to_string().as_bytes(), b":", b"payload"],
        )
        .unwrap();
        let err =
            validate_timestamp_hash(expired_ts, "payload", &expired_sig, "secret").unwrap_err();
        assert!(err.to_string().contains("expired"));

        // 未来时间戳同样受限，避免用远期 ts 换一个长期有效的签名
        let future_ts = ts + (SIGNATURE_TTL_SECS + 1);
        let future_sig = hmac_sha256(
            b"secret",
            &[future_ts.to_string().as_bytes(), b":", b"payload"],
        )
        .unwrap();
        assert!(validate_timestamp_hash(future_ts, "payload", &future_sig, "secret").is_err());

        // 篡改 payload 或时间戳都应失败（ts 参与签名）
        assert!(validate_timestamp_hash(ts, "payloadX", &sig, "secret").is_err());
        assert!(validate_timestamp_hash(ts + 1, "payload", &sig, "secret").is_err());
    }

    /// 签名失败统一是 401，而非默认回退的 500。
    #[test]
    fn signature_failures_are_unauthorized() {
        let err = validate_sign_hash("payload", &"0".repeat(64), "secret").unwrap_err();
        assert_eq!(err.status(), 401);

        let err = validate_timestamp_hash(0, "payload", &"0".repeat(64), "secret").unwrap_err();
        assert_eq!(err.status(), 401);
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
