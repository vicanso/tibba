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

//! 落库字段的对称加密：AES-256-GCM。
//!
//! 用于「必须可解密读回、又不能明文落库」的数据——TOTP 密钥、OAuth refresh
//! token、手机号等 PII。不可逆的凭据（口令）走 [`crate::PasswordPolicy`]，不要
//! 用本模块。
//!
//! ## 落库格式
//! `base64(nonce[12] || ciphertext || tag)`。每次加密取全新随机 nonce；GCM 同时
//! 提供机密性与完整性，解密时自动校验 tag，密文被篡改会得到
//! [`Error::Decrypt`]，不会返回垃圾明文。
//!
//! ## 密钥派生与用途隔离
//! [`SecretCipher::from_app_secret`] 用 `SHA256(app_secret || ":" || domain)`
//! 派生密钥，复用应用已有的强 `secret`，部署侧无需再配一把加密密钥。
//!
//! `domain` 不是可有可无的装饰：**不同用途必须用不同 domain**。否则同一把密钥
//! 加出来的密文可以跨字段互换——把用户 A 的「加密手机号」塞进「加密 TOTP 密钥」
//! 字段，GCM 校验照样通过，因为它验证的是「这段密文没被改过」，而不是「这段密文
//! 属于这个字段」。需要进一步绑定到具体行时用
//! [`SecretCipher::encrypt_with_aad`]。
//!
//! ## ⚠️ secret 轮换
//! 密钥派生自 `app_secret`，轮换 secret 会让所有既有密文**无法解密**。轮换前需
//! 要先用旧 secret 解密、再用新 secret 加密重写，或者接受相应数据失效（例如要求
//! 用户重新绑定 2FA）。

use crate::{Base64Snafu, BlobTooShortSnafu, DecryptSnafu, EncryptSnafu, Error};
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};
use snafu::{OptionExt, ResultExt, ensure};
use zeroize::{Zeroize, Zeroizing};

type Result<T, E = Error> = std::result::Result<T, E>;

/// GCM 推荐的 nonce 长度（96 bit）。
const NONCE_LEN: usize = 12;
/// GCM 认证 tag 长度（128 bit）。
const TAG_LEN: usize = 16;

/// AES-256-GCM 字段加密器（预展开 AES 轮密钥并可选持有历史轮换密钥）。
///
/// 构造方式二选一：[`Self::from_app_secret`]（由应用 secret 派生，最常用）或
/// [`Self::from_key`]（外部密钥管理系统下发的 32 字节密钥）；可通过
/// [`Self::with_fallback_app_secret`] / [`Self::with_fallback_key`] 追加旧密钥以支持平滑轮换。
pub struct SecretCipher {
    primary: Aes256Gcm,
    fallbacks: Vec<Aes256Gcm>,
}

/// 不输出密钥。默认 derive 会把整把密钥打进日志 / panic 回溯。
impl std::fmt::Debug for SecretCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretCipher")
            .field("key", &"<redacted>")
            .field("fallbacks", &self.fallbacks.len())
            .finish()
    }
}

/// 由应用 secret 与用途域派生 32 字节密钥：`SHA256(secret || ":" || domain)`。
/// 保留原始派生协议以保证与既有 TOTP 等落库密文完全兼容。
fn derive_key(secret: &str, domain: &str) -> Zeroizing<[u8; 32]> {
    let mut h = Sha256::new();
    h.update(secret.as_bytes());
    h.update(b":");
    h.update(domain.as_bytes());
    Zeroizing::new(h.finalize().into())
}

fn build_aes_gcm(mut key: [u8; 32]) -> Aes256Gcm {
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(key));
    key.zeroize();
    cipher
}

impl SecretCipher {
    /// 由应用 secret 与用途域派生密钥：`SHA256(secret || ":" || domain)`。
    ///
    /// `domain` 用于隔离不同用途（`"totp"` / `"oauth_refresh"` / `"pii"` …），
    /// 详见模块文档——**不要**在多种数据上复用同一个 domain。
    #[must_use]
    pub fn from_app_secret(secret: &str, domain: &str) -> Self {
        let key = derive_key(secret, domain);
        Self::from_key(*key)
    }

    /// 直接以 32 字节密钥构造，供密钥由外部 KMS / 配置下发的部署使用。
    #[must_use]
    pub fn from_key(key: [u8; 32]) -> Self {
        Self {
            primary: build_aes_gcm(key),
            fallbacks: Vec::new(),
        }
    }

    /// 追加历史应用 secret 作为解密回退密钥（轮换期平滑解密旧密文），支持链式调用。
    #[must_use]
    pub fn with_fallback_app_secret(self, secret: &str, domain: &str) -> Self {
        let key = derive_key(secret, domain);
        self.with_fallback_key(*key)
    }

    /// 追加历史 32 字节原始密钥作为解密回退密钥，支持链式调用。
    #[must_use]
    pub fn with_fallback_key(mut self, key: [u8; 32]) -> Self {
        self.fallbacks.push(build_aes_gcm(key));
        self
    }

    /// 加密明文，返回 `base64(nonce || ciphertext)`。
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        self.encrypt_with_aad(plaintext, &[])
    }

    /// 带附加认证数据（AAD）的加密。
    ///
    /// AAD **不会**被加密，但会参与完整性校验：解密时必须传入完全相同的 AAD，
    /// 否则校验失败。用它把密文绑定到具体记录（如用户 ID），这样即使攻击者能
    /// 写库，也无法把 A 行的密文搬到 B 行——[`Self::encrypt`] 本身挡不住这个。
    pub fn encrypt_with_aad(&self, plaintext: &[u8], aad: &[u8]) -> Result<String> {
        let mut nonce = [0u8; NONCE_LEN];
        // 直接取系统熵源：nonce 重用会彻底摧毁 GCM 的安全性，不能用任何可预测的
        // 伪随机源。走 getrandom 而非 rand——rand_core 0.10 起已不再提供 OsRng，
        // 而 password-hash 生成盐用的也是 getrandom 这条路径。
        getrandom::fill(&mut nonce).ok().context(EncryptSnafu)?;

        // aes_gcm::Error 刻意不透出细节（避免成为 oracle），故归一为 Encrypt
        let ciphertext = self
            .primary
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .ok()
            .context(EncryptSnafu)?;

        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);
        Ok(STANDARD.encode(blob))
    }

    /// 解密 [`Self::encrypt`] 的产物。
    pub fn decrypt(&self, blob_b64: &str) -> Result<Vec<u8>> {
        self.decrypt_with_aad(blob_b64, &[])
    }

    /// 解密 [`Self::encrypt_with_aad`] 的产物，`aad` 必须与加密时完全一致。
    pub fn decrypt_with_aad(&self, blob_b64: &str, aad: &[u8]) -> Result<Vec<u8>> {
        let (plaintext, _rotated) = self.decrypt_and_check_rotation_with_aad(blob_b64, aad)?;
        Ok(plaintext)
    }

    /// 解密并返回 `(明文, 是否命中了回退旧密钥)`，便于调用方按需用新主密钥重加密写回。
    pub fn decrypt_and_check_rotation(&self, blob_b64: &str) -> Result<(Vec<u8>, bool)> {
        self.decrypt_and_check_rotation_with_aad(blob_b64, &[])
    }

    /// 带 AAD 的解密并检测是否命中回退密钥，返回 `(明文, 是否需重加密)`。
    pub fn decrypt_and_check_rotation_with_aad(
        &self,
        blob_b64: &str,
        aad: &[u8],
    ) -> Result<(Vec<u8>, bool)> {
        let blob = STANDARD.decode(blob_b64).context(Base64Snafu)?;
        // AES-GCM 密文至少包含 12 字节 nonce 与 16 字节 tag（即使明文为空）
        ensure!(blob.len() >= NONCE_LEN + TAG_LEN, BlobTooShortSnafu);

        let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
        // split_at 保证长度恰为 NONCE_LEN，转换不会失败；
        // 万一失败按解密失败处理，不 panic
        let nonce = <&Nonce<_>>::try_from(nonce).ok().context(DecryptSnafu)?;

        if let Ok(plaintext) = self.primary.decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        ) {
            return Ok((plaintext, false));
        }
        for fallback in &self.fallbacks {
            if let Ok(plaintext) = fallback.decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad,
                },
            ) {
                return Ok((plaintext, true));
            }
        }
        Err(Error::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn cipher() -> SecretCipher {
        SecretCipher::from_app_secret("app-secret", "test")
    }

    #[test]
    fn round_trip_preserves_plaintext() {
        let c = cipher();
        for plaintext in [b"".as_slice(), b"hello", &[0u8; 1024]] {
            let blob = c.encrypt(plaintext).expect("加密");
            assert_eq!(c.decrypt(&blob).expect("解密"), plaintext);
        }
    }

    /// nonce 必须每次都不同——重用 nonce 会彻底摧毁 GCM 的安全性。
    #[test]
    fn each_encryption_uses_a_fresh_nonce() {
        let c = cipher();
        let a = c.encrypt(b"same input").expect("加密");
        let b = c.encrypt(b"same input").expect("加密");
        assert_ne!(a, b, "相同明文两次加密不得产生相同密文");
        // 但都要能解回同一个明文
        assert_eq!(c.decrypt(&a).expect("解密"), b"same input");
        assert_eq!(c.decrypt(&b).expect("解密"), b"same input");
    }

    /// 篡改密文必须被 GCM 的 tag 校验拦下，而不是返回垃圾明文。
    #[test]
    fn tampered_ciphertext_is_rejected() {
        let c = cipher();
        let blob = c.encrypt(b"sensitive").expect("加密");
        let mut raw = STANDARD.decode(&blob).expect("解码");
        // 翻转密文区的一个 bit（跳过 nonce）
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        let tampered = STANDARD.encode(raw);

        assert!(matches!(c.decrypt(&tampered).unwrap_err(), Error::Decrypt));
    }

    #[test]
    fn malformed_blob_is_rejected() {
        let c = cipher();
        assert!(matches!(
            c.decrypt("not base64!!").unwrap_err(),
            Error::Base64 { .. }
        ));
        // 长度不足以容纳 nonce + tag（< 28 字节）
        assert!(matches!(
            c.decrypt(&STANDARD.encode([0u8; NONCE_LEN])).unwrap_err(),
            Error::BlobTooShort
        ));
        assert!(matches!(
            c.decrypt(&STANDARD.encode([0u8; NONCE_LEN + TAG_LEN - 1]))
                .unwrap_err(),
            Error::BlobTooShort
        ));
        assert!(matches!(c.decrypt("").unwrap_err(), Error::BlobTooShort));
    }

    #[test]
    fn fallback_keys_support_graceful_secret_rotation() {
        let old = SecretCipher::from_app_secret("old-secret", "totp");
        let rotated = SecretCipher::from_app_secret("new-secret", "totp")
            .with_fallback_app_secret("old-secret", "totp");

        let old_blob = old.encrypt_with_aad(b"totp-seed", b"user:1").expect("加密");
        let new_blob = rotated
            .encrypt_with_aad(b"new-seed", b"user:1")
            .expect("加密");

        let (pt, needs_reencrypt) = rotated
            .decrypt_and_check_rotation_with_aad(&old_blob, b"user:1")
            .expect("命中旧密钥");
        assert_eq!(pt, b"totp-seed");
        assert!(needs_reencrypt);

        let (pt_new, needs_reencrypt_new) = rotated
            .decrypt_and_check_rotation_with_aad(&new_blob, b"user:1")
            .expect("命中新主密钥");
        assert_eq!(pt_new, b"new-seed");
        assert!(!needs_reencrypt_new);
    }

    /// **用途隔离**：domain 不同即密钥不同，密文不可跨用途解密。
    ///
    /// 少了这一条，同一把密钥加出来的密文可以在字段之间互换而校验照样通过。
    #[test]
    fn different_domains_cannot_decrypt_each_other() {
        let totp = SecretCipher::from_app_secret("app-secret", "totp");
        let pii = SecretCipher::from_app_secret("app-secret", "pii");

        let blob = totp.encrypt(b"secret").expect("加密");
        assert!(matches!(pii.decrypt(&blob).unwrap_err(), Error::Decrypt));
    }

    /// 换 app secret 同样解不开（轮换语义，见模块文档）。
    #[test]
    fn rotated_app_secret_cannot_decrypt() {
        let old = SecretCipher::from_app_secret("old-secret", "totp");
        let new = SecretCipher::from_app_secret("new-secret", "totp");
        let blob = old.encrypt(b"secret").expect("加密");
        assert!(matches!(new.decrypt(&blob).unwrap_err(), Error::Decrypt));
    }

    /// AAD 必须参与校验：AAD 不一致就解不开，密文因此被绑定到具体记录。
    #[test]
    fn aad_binds_ciphertext_to_its_record() {
        let c = cipher();
        let blob = c.encrypt_with_aad(b"phone", b"user:42").expect("加密");

        assert_eq!(
            c.decrypt_with_aad(&blob, b"user:42").expect("解密"),
            b"phone"
        );
        // 换一个用户 ID：搬运密文的攻击会在这里失败
        assert!(matches!(
            c.decrypt_with_aad(&blob, b"user:43").unwrap_err(),
            Error::Decrypt
        ));
        // 不带 AAD 也解不开
        assert!(matches!(c.decrypt(&blob).unwrap_err(), Error::Decrypt));
    }

    /// `from_key` 与 `from_app_secret` 产出的是同一套格式，可互相解密。
    #[test]
    fn from_key_is_interchangeable_with_derived_key() {
        let mut h = Sha256::new();
        h.update(b"app-secret");
        h.update(b":");
        h.update(b"test");
        let key: [u8; 32] = h.finalize().into();

        let explicit = SecretCipher::from_key(key);
        let blob = cipher().encrypt(b"x").expect("加密");
        assert_eq!(explicit.decrypt(&blob).expect("解密"), b"x");
    }

    /// Debug 不得泄漏密钥。
    #[test]
    fn debug_does_not_leak_key() {
        let debug = format!("{:?}", SecretCipher::from_key([0xAB; 32]));
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(!debug.contains("171"), "不得输出密钥字节: {debug}");
    }
}
