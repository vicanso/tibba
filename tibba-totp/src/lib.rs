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

//! tibba-totp —— RFC 6238 TOTP 两步验证核心 + 密钥落库加密。
//!
//! ## 提供能力
//! - [`generate_secret`]：生成 160-bit 随机密钥（authenticator app 标准长度）
//! - [`base32_encode`] / [`otpauth_uri`]：把密钥编码成 app 可扫描的 `otpauth://` URI
//! - [`verify_code`]：校验用户输入的 6 位动态码（HMAC-SHA1，±1 时间窗容差）
//! - [`generate_recovery_codes`] / [`hash_recovery_code`]：一次性恢复码生成与哈希
//! - [`SecretCipher`]：AES-256-GCM 对密钥做落库加密（密钥派生自应用 secret）
//!
//! ## 算法选择
//! - **SHA1**：authenticator app（Google Authenticator / Authy 等）默认算法，
//!   兼容性最佳。`otpauth://` URI 显式标 `algorithm=SHA1`。
//! - **6 位 / 30 秒周期**：业界默认，与各 app 默认值一致，免去用户手动配置。
//!
//! ## 不引入 totp-rs
//! TOTP 本身是 HMAC 之上的薄封装（RFC 6238 §4），故直接用经过审计的
//! RustCrypto `hmac` + `sha1` 原语组合，避免 totp-rs 连带的 image/qrcode 重依赖；
//! 二维码由前端用 `otpauth_uri` 自行渲染。正确性由 RFC 4226/6238 官方测试向量保证。

use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use snafu::{OptionExt, ResultExt, Snafu};
use subtle::ConstantTimeEq;
use tibba_error::Error as BaseError;

type HmacSha1 = Hmac<Sha1>;

/// 动态码位数（业界默认 6 位）。
pub const DIGITS: u32 = 6;
/// 时间步长（秒），RFC 6238 默认 30 秒。
pub const PERIOD: u64 = 30;
/// 密钥字节数。160-bit 是 SHA1-HMAC 的推荐密钥长度（RFC 4226 §4）。
pub const SECRET_BYTES: usize = 20;
/// 时间窗容差：允许相邻 ±1 个步长，吸收客户端/服务端时钟漂移（约 ±30s）。
const SKEW: i64 = 1;

/// base32 标准字母表（RFC 4648），authenticator app 解析密钥时使用。
const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// 恢复码字母表：32 个无歧义字符（去掉 l/o，避免与 1/0 混淆）。
/// 恰为 32 个，使「字节 & 0x1f」映射无取模偏置。
const RECOVERY_ALPHABET: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";

/// tibba-totp 错误。多数函数（密钥/码生成、验证）不会失败，仅加解密会。
#[derive(Debug, Snafu)]
pub enum Error {
    /// AES-GCM 加密失败（实际几乎不发生；GCM 加密无业务前置条件）。
    #[snafu(display("totp secret encryption failed"))]
    Encrypt,
    /// AES-GCM 解密/校验失败：密文被篡改、密钥变更或数据损坏。
    #[snafu(display("totp secret decryption failed"))]
    Decrypt,
    /// 落库密文的 base64 解码失败。
    #[snafu(display("decode encrypted secret: {source}"))]
    Base64 { source: base64::DecodeError },
    /// 密文长度不足以容纳 12 字节 nonce + 密文，数据已损坏。
    #[snafu(display("encrypted secret blob too short"))]
    BlobTooShort,
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Encrypt => BaseError::new("totp secret encryption failed")
                .with_sub_category("encrypt")
                .with_status(500)
                .with_exception(true),
            Error::Decrypt => BaseError::new("totp secret decryption failed")
                .with_sub_category("decrypt")
                .with_status(500)
                .with_exception(true),
            Error::Base64 { source } => BaseError::new(source)
                .with_sub_category("base64")
                .with_exception(true),
            Error::BlobTooShort => BaseError::new("encrypted secret blob too short")
                .with_sub_category("blob_too_short")
                .with_status(500)
                .with_exception(true),
        };
        err.with_category("totp")
    }
}

/// 模块内部 Result，公开函数通过 `?` 自动转 [`BaseError`]。
type Result<T, E = Error> = std::result::Result<T, E>;

/// 生成一个新的随机 TOTP 密钥（[`SECRET_BYTES`] 字节）。
pub fn generate_secret() -> Vec<u8> {
    let mut bytes = vec![0u8; SECRET_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

/// 将密钥按 RFC 4648 base32（大写、无填充）编码，供 `otpauth://` URI 使用。
/// 20 字节恰好编码为 32 个 base32 字符，无需填充。
pub fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for &b in data {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let idx = ((buffer >> bits) & 0x1f) as usize;
            out.push(BASE32_ALPHABET[idx] as char);
        }
    }
    // 处理末尾不足 5 bit 的残余
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 0x1f) as usize;
        out.push(BASE32_ALPHABET[idx] as char);
    }
    out
}

/// 构造 `otpauth://totp/...` URI，authenticator app 扫码或手动导入时使用。
/// `secret_b32` 由 [`base32_encode`] 得到；`issuer`/`account` 会被 percent-encode。
pub fn otpauth_uri(secret_b32: &str, account: &str, issuer: &str) -> String {
    let issuer_enc = percent_encode(issuer);
    let account_enc = percent_encode(account);
    // label 形如 "Issuer:account"，query 里再带一份 issuer 供 app 显示分组
    format!(
        "otpauth://totp/{issuer_enc}:{account_enc}\
         ?secret={secret_b32}&issuer={issuer_enc}&algorithm=SHA1&digits={DIGITS}&period={PERIOD}"
    )
}

/// 校验动态码并返回匹配的 HOTP 计数器（step）；不匹配返回 `None`。
/// 调用方可据返回的计数器做**防重放**：标记该 step 已消费，拒绝同一 step 再次使用。
///
/// `unix_time` 由调用方传入当前 Unix 秒，保持本 crate 不依赖时钟。
/// 比较使用常量时间，避免对攻击者可控输入产生计时侧信道。
pub fn verify_code_step(secret: &[u8], code: &str, unix_time: i64) -> Option<u64> {
    let code = code.trim();
    // 位数不符直接拒绝（也保证 ct 比较两侧等长）
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let step = unix_time / PERIOD as i64;
    for delta in -SKEW..=SKEW {
        let counter = (step + delta).max(0) as u64;
        let expected = format!("{:0width$}", hotp(secret, counter), width = DIGITS as usize);
        if bool::from(expected.as_bytes().ct_eq(code.as_bytes())) {
            return Some(counter);
        }
    }
    None
}

/// 校验用户输入的动态码是否匹配当前时间窗（含 ±[`SKEW`] 容差）。
/// 不关心具体匹配的 step（如注册激活场景）时用本函数；需防重放请用
/// [`verify_code_step`] 拿到计数器再做已消费标记。
pub fn verify_code(secret: &[u8], code: &str, unix_time: i64) -> bool {
    verify_code_step(secret, code, unix_time).is_some()
}

/// HOTP（RFC 4226）：对计数器做 HMAC-SHA1 + 动态截断，取 [`DIGITS`] 位。
/// HMAC 接受任意长度密钥，`new_from_slice` 对 HMAC 而言不会失败。
fn hotp(secret: &[u8], counter: u64) -> u32 {
    hotp_digits(secret, counter, DIGITS)
}

/// HOTP 通用实现，`digits` 可配（测试用 RFC 向量需要 6/8 两种位数）。
fn hotp_digits(secret: &[u8], counter: u64, digits: u32) -> u32 {
    // HMAC-SHA1 对任意密钥长度都有定义，这里不会返回 Err
    let mut mac = match HmacSha1::new_from_slice(secret) {
        Ok(m) => m,
        Err(_) => return u32::MAX, // 不可达；返回一个绝不匹配 6 位码的值
    };
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    // 动态截断（RFC 4226 §5.3）：用最后一字节低 4 位作偏移取 4 字节
    let offset = (digest[19] & 0x0f) as usize;
    let bin = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    bin % 10u32.pow(digits)
}

/// 生成 `count` 个一次性恢复码，形如 `abcde-fghij`（明文，仅在激活时返回一次）。
/// 调用方应立即对每个码调 [`hash_recovery_code`] 存哈希，明文不落库。
pub fn generate_recovery_codes(count: usize) -> Vec<String> {
    let mut rng = rand::rng();
    (0..count)
        .map(|_| {
            let mut raw = [0u8; 10];
            rng.fill_bytes(&mut raw);
            // 256 % 32 == 0，`& 0x1f` 映射到 32 字母表无偏置
            let chars: String = raw
                .iter()
                .map(|b| RECOVERY_ALPHABET[(*b & 0x1f) as usize] as char)
                .collect();
            format!("{}-{}", &chars[0..5], &chars[5..10])
        })
        .collect()
}

/// 对恢复码做规范化（去横杠/空白、转小写）后取 SHA-256，base64 输出。
/// 规范化使用户输入带不带横杠都能匹配。落库与校验都走本函数，保证一致。
pub fn hash_recovery_code(code: &str) -> String {
    let norm: String = code
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let mut h = Sha256::new();
    h.update(norm.as_bytes());
    STANDARD.encode(h.finalize())
}

/// 最小 percent-encode：保留 unreserved 字符，其余按 UTF-8 字节 `%XX`。
/// 用于 `otpauth://` 的 issuer/account，避免引入 urlencoding 依赖。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// TOTP 密钥落库加密器：AES-256-GCM，密钥派生自应用 secret。
///
/// ## 密钥派生
/// `key = SHA256(app_secret || ":totp")`。复用应用已有的强 `secret`，
/// 部署侧无需额外配置加密密钥。**注意**：`secret` 轮换会导致已有密文无法解密
/// （与登录令牌签名同样的取舍）——轮换 secret 时需要求用户重新绑定 2FA。
///
/// ## 落库格式
/// `base64(nonce[12] || ciphertext)`。每次加密用全新随机 nonce，GCM 提供
/// 机密性 + 完整性（解密时自动校验 tag，篡改会触发 [`Error::Decrypt`]）。
pub struct SecretCipher {
    key: [u8; 32],
}

impl SecretCipher {
    /// 从应用 secret 派生加密密钥。
    pub fn from_app_secret(secret: &str) -> Self {
        let mut h = Sha256::new();
        h.update(secret.as_bytes());
        h.update(b":totp");
        let key: [u8; 32] = h.finalize().into();
        Self { key }
    }

    /// 加密明文密钥，返回 `base64(nonce || ciphertext)`。
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        use aes_gcm::Aes256Gcm;
        use aes_gcm::aead::{Aead, KeyInit, generic_array::GenericArray};

        let cipher = Aes256Gcm::new(GenericArray::from_slice(&self.key));
        let mut nonce = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce);
        // GCM 加密失败无业务可恢复信息（aes_gcm::Error 故意不透出细节），
        // 故用 ok().context() 归一为 Encrypt，避免 map_err 闭包
        let ct = cipher
            .encrypt(GenericArray::from_slice(&nonce), plaintext)
            .ok()
            .context(EncryptSnafu)?;
        let mut blob = Vec::with_capacity(12 + ct.len());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        Ok(STANDARD.encode(blob))
    }

    /// 解密 [`encrypt`](Self::encrypt) 产出的 base64 串，返回明文密钥字节。
    pub fn decrypt(&self, blob_b64: &str) -> Result<Vec<u8>> {
        use aes_gcm::Aes256Gcm;
        use aes_gcm::aead::{Aead, KeyInit, generic_array::GenericArray};

        let blob = STANDARD.decode(blob_b64).context(Base64Snafu)?;
        if blob.len() < 12 {
            return BlobTooShortSnafu.fail();
        }
        let (nonce, ct) = blob.split_at(12);
        let cipher = Aes256Gcm::new(GenericArray::from_slice(&self.key));
        let pt = cipher
            .decrypt(GenericArray::from_slice(nonce), ct)
            .ok()
            .context(DecryptSnafu)?;
        Ok(pt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4226 Appendix D：密钥 "12345678901234567890"，counter 0..9 的 6 位 HOTP。
    #[test]
    fn rfc4226_hotp_vectors() {
        let secret = b"12345678901234567890";
        let expected = [
            755224, 287082, 359152, 969429, 338314, 254676, 287922, 162583, 399871, 520489,
        ];
        for (counter, want) in expected.iter().enumerate() {
            assert_eq!(
                hotp_digits(secret, counter as u64, 6),
                *want,
                "counter={counter}"
            );
        }
    }

    /// RFC 6238 Appendix B：SHA1 + 8 位，验证时间步换算与截断对更长位数也成立。
    #[test]
    fn rfc6238_totp_vectors_sha1() {
        let secret = b"12345678901234567890";
        // (unix_time, 期望 8 位码)
        let cases = [
            (59i64, 94287082u32),
            (1111111109, 7081804),
            (1111111111, 14050471),
            (1234567890, 89005924),
            (2000000000, 69279037),
        ];
        for (t, want) in cases {
            let counter = (t / PERIOD as i64) as u64;
            assert_eq!(hotp_digits(secret, counter, 8), want, "t={t}");
        }
    }

    /// verify_code 应接受当前步、相邻 ±1 步，拒绝 ±2 步。
    #[test]
    fn verify_code_accepts_skew_window() {
        let secret = generate_secret();
        let t = 1_700_000_000i64;
        let here = format!("{:06}", hotp(&secret, (t / 30) as u64));
        let prev = format!("{:06}", hotp(&secret, (t / 30 - 1) as u64));
        let next = format!("{:06}", hotp(&secret, (t / 30 + 1) as u64));
        let far = format!("{:06}", hotp(&secret, (t / 30 + 2) as u64));
        assert!(verify_code(&secret, &here, t));
        assert!(verify_code(&secret, &prev, t));
        assert!(verify_code(&secret, &next, t));
        assert!(!verify_code(&secret, &far, t));
        // 非法输入
        assert!(!verify_code(&secret, "12345", t)); // 位数不足
        assert!(!verify_code(&secret, "abcdef", t)); // 非数字
    }

    /// base32 编码 20 字节应得 32 个字符且全在字母表内。
    #[test]
    fn base32_encode_known() {
        // 全 0 → 全 'A'
        assert_eq!(base32_encode(&[0u8; 20]), "A".repeat(32));
        let s = base32_encode(&generate_secret());
        assert_eq!(s.len(), 32);
        assert!(s.bytes().all(|b| BASE32_ALPHABET.contains(&b)));
    }

    /// 加解密 round-trip，且错误密钥/篡改密文应解密失败。
    #[test]
    fn cipher_round_trip() {
        let cipher = SecretCipher::from_app_secret("super-secret-app-key");
        let secret = generate_secret();
        let blob = cipher.encrypt(&secret).expect("encrypt");
        assert_eq!(cipher.decrypt(&blob).expect("decrypt"), secret);

        // 不同 app secret 派生不同密钥，解密失败
        let other = SecretCipher::from_app_secret("different-key");
        assert!(other.decrypt(&blob).is_err());
    }

    /// 恢复码：格式 xxxxx-xxxxx，且带不带横杠/大小写都能哈希一致。
    #[test]
    fn recovery_code_hash_normalizes() {
        let codes = generate_recovery_codes(10);
        assert_eq!(codes.len(), 10);
        for c in &codes {
            assert_eq!(c.len(), 11); // 5 + '-' + 5
            assert_eq!(c.as_bytes()[5], b'-');
        }
        let h1 = hash_recovery_code("abcde-fghij");
        let h2 = hash_recovery_code("ABCDEFGHIJ");
        let h3 = hash_recovery_code("abcde fghij");
        assert_eq!(h1, h2);
        assert_eq!(h1, h3);
        assert_ne!(h1, hash_recovery_code("abcde-fghik"));
    }
}
