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

use super::{Error, HmacSha256Snafu};
use arc_swap::ArcSwap;
use hex::encode;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use snafu::ResultExt;
use std::sync::Arc;

type Result<T> = std::result::Result<T, Error>;

/// HMAC-SHA256 类型别名。
type HmacSha256 = Hmac<Sha256>;

/// 基于 HMAC-SHA256 的多密钥管理器，支持签名、验签与密钥轮换。
///
/// 内部使用 `ArcSwap` 存储密钥列表：读取（`sign`/`verify`）走无锁原子加载，
/// 写入（`update_keys`）通过原子替换实现热轮换。第一个密钥始终是当前主密钥，
/// 后续密钥用于校验旧签名的有效性以支持平滑过渡。
///
/// 类型不变量：密钥列表始终非空（由 [`Self::new`] / [`Self::update_keys`] 保证）。
pub struct KeyGrip {
    keys: ArcSwap<Vec<Vec<u8>>>,
}

/// 手写 `Debug`：derive 会把**签名密钥原文**逐字节打出来。
///
/// `KeyGrip` 常被放进别的结构（例如 webhook 的投递器），只要外层有人
/// `derive(Debug)` 再 `{:?}` 一下，密钥就进了日志或 panic 回溯。这里只暴露
/// 密钥个数——轮换排查时唯一有用、又不泄密的信息。同 `SecretCipher` 的处理。
impl std::fmt::Debug for KeyGrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyGrip")
            .field(
                "keys",
                &format_args!("<{} redacted>", self.keys.load().len()),
            )
            .finish()
    }
}

/// 使用指定密钥对数据进行 HMAC-SHA256 签名，返回十六进制编码的签名字符串。
fn sign_with_key(data: &[u8], key: &[u8]) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(key).context(HmacSha256Snafu)?;
    mac.update(data);
    Ok(encode(mac.finalize().into_bytes()))
}

/// 使用指定密钥对数据签名并以**常量时间**比较预期摘要字节，匹配返回 true。
///
/// 直接用 `==` 比较 hex 字符串会泄露逐字节比较的时序差异，
/// 攻击者可借此推断签名前缀；改走 `hmac::Mac::verify_slice` 保证常数时间。
fn verify_with_key(data: &[u8], key: &[u8], expected: &[u8]) -> Result<bool> {
    let mut mac = HmacSha256::new_from_slice(key).context(HmacSha256Snafu)?;
    mac.update(data);
    Ok(mac.verify_slice(expected).is_ok())
}

impl KeyGrip {
    /// 创建 KeyGrip 实例；`keys` 为空时返回 [`Error::KeyGripEmpty`]。
    /// 默认即支持运行时密钥轮换，无需区分静态/共享模式。
    pub fn new(keys: Vec<Vec<u8>>) -> Result<Self> {
        if keys.is_empty() {
            return Err(Error::KeyGripEmpty);
        }
        Ok(Self {
            keys: ArcSwap::from_pointee(keys),
        })
    }

    /// 原子替换密钥列表，用于密钥轮换；`new_keys` 为空时返回错误以维持非空不变量。
    pub fn update_keys(&self, new_keys: Vec<Vec<u8>>) -> Result<()> {
        if new_keys.is_empty() {
            return Err(Error::KeyGripEmpty);
        }
        self.keys.store(Arc::new(new_keys));
        Ok(())
    }

    /// 使用当前主密钥（索引 0）对数据签名，返回十六进制编码的 HMAC-SHA256 签名。
    pub fn sign(&self, data: &[u8]) -> Result<String> {
        let keys = self.keys.load();
        // 类型不变量保证 keys 非空，first() 一定有值
        let key = keys.first().ok_or(Error::KeyGripEmpty)?;
        sign_with_key(data, key)
    }

    /// 验证签名，返回匹配到的是哪一代密钥。
    ///
    /// 此前返回 `(is_valid, is_current)` 两个布尔——调用方得记住哪个是哪个，
    /// 而且 `(false, true)` 这种不可能的组合在类型上也合法。改成枚举后三种
    /// 状态一目了然，也不存在非法组合。
    ///
    /// 比较走常数时间路径（[`verify_with_key`]）；`digest` 非合法 hex 字符串时
    /// 视为 [`Verification::Invalid`]，不向上游报错。
    pub fn verify(&self, data: &[u8], digest: &str) -> Result<Verification> {
        // hex 解码失败说明客户端送了非法签名，等同于不匹配；不向上游报错
        let Ok(expected) = hex::decode(digest) else {
            return Ok(Verification::Invalid);
        };

        let keys = self.keys.load();
        for (index, key) in keys.iter().enumerate() {
            if verify_with_key(data, key, &expected)? {
                return Ok(if index == 0 {
                    Verification::Current
                } else {
                    Verification::Rotated
                });
            }
        }
        Ok(Verification::Invalid)
    }
}

/// [`KeyGrip::verify`] 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// 与当前主密钥匹配。
    Current,
    /// 与某个历史密钥匹配：签名仍然有效，但调用方应借机用主密钥**重新签发**，
    /// 让旧密钥早日退役。
    Rotated,
    /// 与任何密钥都不匹配（或签名格式非法）。
    Invalid,
}

impl Verification {
    /// 签名是否有效（当前或历史密钥皆可）。
    #[must_use]
    pub fn is_valid(self) -> bool {
        !matches!(self, Self::Invalid)
    }

    /// 是否需要用主密钥重新签发。
    #[must_use]
    pub fn needs_resign(self) -> bool {
        matches!(self, Self::Rotated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn keys(primary: &[u8], rest: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut v = vec![primary.to_vec()];
        v.extend(rest.iter().map(|k| k.to_vec()));
        v
    }

    #[test]
    fn empty_keys_rejected() {
        assert!(matches!(
            KeyGrip::new(Vec::new()).unwrap_err(),
            Error::KeyGripEmpty
        ));
    }

    #[test]
    fn sign_then_verify_with_primary() {
        let kg = KeyGrip::new(keys(b"primary", &[])).unwrap();
        let sig = kg.sign(b"hello").unwrap();
        assert_eq!(sig.len(), 64); // 32 bytes * 2 hex chars
        assert_eq!(kg.verify(b"hello", &sig).unwrap(), Verification::Current);
    }

    #[test]
    fn verify_with_rotated_key_marks_not_current() {
        // 主密钥换成 primary，旧密钥 legacy 仍能验签，但结果是 Rotated
        let kg = KeyGrip::new(keys(b"primary", &[b"legacy"])).unwrap();
        let legacy_sig = sign_with_key(b"hello", b"legacy").unwrap();
        assert_eq!(
            kg.verify(b"hello", &legacy_sig).unwrap(),
            Verification::Rotated,
            "签名匹配历史密钥应返回 Rotated（提示调用方重新签名）"
        );
    }

    #[test]
    fn verify_unknown_signature_returns_invalid() {
        let kg = KeyGrip::new(keys(b"primary", &[])).unwrap();
        let foreign_sig = sign_with_key(b"hello", b"someone-else").unwrap();
        assert_eq!(
            kg.verify(b"hello", &foreign_sig).unwrap(),
            Verification::Invalid
        );
    }

    #[test]
    fn verify_malformed_hex_returns_invalid_not_error() {
        let kg = KeyGrip::new(keys(b"primary", &[])).unwrap();
        // "zzz" 既不是合法 hex 也长度不对——应当视为无效签名而非传播错误
        assert_eq!(kg.verify(b"hello", "zzz").unwrap(), Verification::Invalid);
        assert_eq!(kg.verify(b"hello", "").unwrap(), Verification::Invalid);
    }

    #[test]
    fn update_keys_atomically_rotates_primary() {
        let kg = KeyGrip::new(keys(b"v1", &[])).unwrap();
        let old_sig = kg.sign(b"payload").unwrap();
        assert_eq!(
            kg.verify(b"payload", &old_sig).unwrap(),
            Verification::Current
        );

        // 轮换：v2 升为主密钥，v1 沦为历史密钥
        kg.update_keys(keys(b"v2", &[b"v1"])).unwrap();

        let new_sig = kg.sign(b"payload").unwrap();
        assert_ne!(new_sig, old_sig, "新主密钥应产生不同签名");
        assert_eq!(
            kg.verify(b"payload", &new_sig).unwrap(),
            Verification::Current,
            "新签名应匹配主密钥"
        );
        assert_eq!(
            kg.verify(b"payload", &old_sig).unwrap(),
            Verification::Rotated,
            "旧签名应仍有效但标记为非当前"
        );
    }

    #[test]
    fn update_keys_rejects_empty() {
        let kg = KeyGrip::new(keys(b"v1", &[])).unwrap();
        assert!(matches!(
            kg.update_keys(Vec::new()).unwrap_err(),
            Error::KeyGripEmpty
        ));
        // 失败的 update 不应影响原密钥
        let sig = kg.sign(b"x").unwrap();
        assert_eq!(kg.verify(b"x", &sig).unwrap(), Verification::Current);
    }

    /// Debug 不得泄漏密钥。回归到 derive(Debug) 时本例会失败。
    #[test]
    fn debug_does_not_leak_keys() {
        let kg = KeyGrip::new(keys(b"super-secret-key", &[b"old-secret"])).unwrap();
        let debug = format!("{kg:?}");
        assert!(!debug.contains("super-secret-key"), "{debug}");
        // derive 出来的形态是字节数组，把首字节 's'(115) 也排除掉
        assert!(!debug.contains("115"), "{debug}");
        assert!(debug.contains("<2 redacted>"), "{debug}");
    }

    #[test]
    fn binary_payload_is_supported() {
        let kg = KeyGrip::new(keys(&[0xDE, 0xAD, 0xBE, 0xEF], &[])).unwrap();
        let data = &[0x00, 0xFF, 0x42, 0x00, 0x7F];
        let sig = kg.sign(data).unwrap();
        assert_eq!(kg.verify(data, &sig).unwrap(), Verification::Current);
        // 篡改后不应匹配
        assert_eq!(
            kg.verify(&[0x00, 0xFF, 0x42, 0x00, 0x80], &sig).unwrap(),
            Verification::Invalid
        );
    }
}
