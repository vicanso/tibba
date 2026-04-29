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

use super::{Error, HmacSha256Snafu};
use hex::encode;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use snafu::ResultExt;
use std::sync::Arc;
use std::sync::RwLock;

type Result<T> = std::result::Result<T, Error>;

/// HMAC-SHA256 类型别名。
type HmacSha256 = Hmac<Sha256>;

/// 密钥存储方式：静态（单线程）或共享（多线程，支持热更新）。
enum KeyStore {
    /// 不可变密钥列表，适用于无需热更新的场景。
    Static(Vec<Vec<u8>>),
    /// 通过 Arc<RwLock> 共享的密钥列表，支持运行时更新。
    Shared(Arc<RwLock<Vec<Vec<u8>>>>),
}

/// 基于 HMAC-SHA256 的多密钥管理器，支持签名、验签与密钥轮换。
/// 同时提供静态（`new`）和线程安全（`new_with_lock`）两种构造方式。
pub struct KeyGrip {
    store: KeyStore,
}

/// 使用指定密钥对数据进行 HMAC-SHA256 签名，返回十六进制编码的签名字符串。
fn sign_with_key(data: &[u8], key: &[u8]) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(key).context(HmacSha256Snafu)?;
    mac.update(data);
    Ok(encode(mac.finalize().into_bytes()))
}

impl KeyGrip {
    /// 创建静态密钥存储的 KeyGrip 实例，不支持运行时更新密钥。
    /// `keys` 为空时返回 `KeyGripEmpty` 错误。
    pub fn new(keys: Vec<Vec<u8>>) -> Result<Self> {
        if keys.is_empty() {
            return Err(Error::KeyGripEmpty);
        }
        Ok(Self {
            store: KeyStore::Static(keys),
        })
    }

    /// 创建线程安全（RwLock）密钥存储的 KeyGrip 实例，支持运行时热更新密钥。
    /// `keys` 为空时返回 `KeyGripEmpty` 错误。
    pub fn new_with_lock(keys: Vec<Vec<u8>>) -> Result<Self> {
        if keys.is_empty() {
            return Err(Error::KeyGripEmpty);
        }
        Ok(Self {
            store: KeyStore::Shared(Arc::new(RwLock::new(keys))),
        })
    }

    /// 以只读方式借用内部密钥列表并执行闭包。
    /// 读锁获取失败时传入空切片，实践中不会发生。
    fn with_keys<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[Vec<u8>]) -> R,
    {
        match &self.store {
            KeyStore::Static(keys) => f(keys),
            KeyStore::Shared(lock_keys) => {
                if let Ok(keys) = lock_keys.read() {
                    f(&keys)
                } else {
                    f(&[])
                }
            }
        }
    }

    /// 替换共享存储中的密钥列表，用于密钥轮换。
    /// 静态存储模式下为空操作。
    pub fn update_keys(&self, new_keys: Vec<Vec<u8>>) {
        if let KeyStore::Shared(lock_keys) = &self.store
            && let Ok(mut keys) = lock_keys.write()
        {
            *keys = new_keys;
        }
    }

    /// 遍历所有密钥，找到与给定签名匹配的密钥索引。
    /// 返回 `Some(index)` 表示找到匹配，`None` 表示未找到。
    fn index(&self, data: &[u8], digest: &str) -> Result<Option<usize>> {
        self.with_keys(|keys| {
            for (index, key) in keys.iter().enumerate() {
                match sign_with_key(data, key) {
                    Ok(signature) if signature == digest => return Ok(Some(index)),
                    Ok(_) => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(None)
        })
    }

    /// 使用第一个密钥对数据进行签名，返回十六进制编码的 HMAC-SHA256 签名。
    /// 密钥列表为空时返回 `KeyGripEmpty` 错误。
    pub fn sign(&self, data: &[u8]) -> Result<String> {
        self.with_keys(|keys| {
            let key = keys.first().ok_or(Error::KeyGripEmpty)?;
            sign_with_key(data, key)
        })
    }

    /// 验证签名是否与数据匹配，返回 `(is_valid, is_current)`：
    /// - `is_valid`：签名与任意密钥匹配则为 `true`
    /// - `is_current`：签名与当前主密钥（第一个）匹配则为 `true`
    pub fn verify(&self, data: &[u8], digest: &str) -> Result<(bool, bool)> {
        match self.index(data, digest)? {
            Some(0) => Ok((true, true)),  // 匹配当前主密钥，有效且最新
            Some(_) => Ok((true, false)), // 匹配历史密钥，有效但需轮换
            None => Ok((false, false)),   // 未找到匹配，签名无效
        }
    }
}
