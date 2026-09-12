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

use crate::{Argon2HashSnafu, Error, InvalidParamsSnafu, PhcParseSnafu, SecretTooLongSnafu};
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use snafu::{ResultExt, ensure};
use subtle::ConstantTimeEq;

type Result<T, E = Error> = std::result::Result<T, E>;

/// 允许的 secret 最大字节数。
///
/// Argon2 自身允许近 4 GiB 的输入（`argon2::MAX_PWD_LEN`），而它是**慢**哈希——
/// 超长输入就是一个现成的 CPU 放大器，且登录接口通常无需鉴权即可调用。1 KiB
/// 对任何真实口令都绰绰有余（本项目通常传的是 `sha256(password)` 的 64 字符串）。
pub const DEFAULT_MAX_SECRET_LEN: usize = 1024;

/// 密码校验结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordCheck {
    /// 校验通过，存储已是符合当前策略的 Argon2 哈希，无需额外处理。
    Matched,
    /// 校验通过，但存储形式已过时，调用方应借这次成功登录重新哈希写回（无感升级）。
    ///
    /// 两种情况都归于此：
    /// - 旧式明文 sha256 存量
    /// - Argon2 哈希的代价参数**弱于**当前策略
    MatchedNeedsRehash,
    /// 校验失败（密码不匹配）。
    Mismatch,
}

/// 密码哈希策略：Argon2id 代价参数 + 输入长度上限。
///
/// 参数可按部署环境调整——同一份代码跑在 4 核容器和 32 核物理机上，合适的
/// `m_cost` / `t_cost` 并不相同。调高参数后，存量哈希会在各自下次登录成功时
/// 通过 [`PasswordCheck::MatchedNeedsRehash`] 被逐步升级，无需停机迁移。
///
/// ```ignore
/// let policy = PasswordPolicy::new()
///     .with_m_cost(64 * 1024)   // 64 MiB
///     .with_t_cost(3);
/// let stored = policy.hash(secret)?;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordPolicy {
    /// 内存代价，单位 KiB
    m_cost: u32,
    /// 时间代价（迭代次数）
    t_cost: u32,
    /// 并行度
    p_cost: u32,
    /// 输入 secret 的字节数上限
    max_secret_len: usize,
}

impl Default for PasswordPolicy {
    /// 沿用 argon2 crate 的推荐默认（m=19 MiB, t=2, p=1），叠加长度上限。
    fn default() -> Self {
        Self {
            m_cost: Params::DEFAULT_M_COST,
            t_cost: Params::DEFAULT_T_COST,
            p_cost: Params::DEFAULT_P_COST,
            max_secret_len: DEFAULT_MAX_SECRET_LEN,
        }
    }
}

impl PasswordPolicy {
    /// 使用默认参数，见 [`Self::default`]。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置内存代价（单位 KiB），支持链式调用。
    #[must_use]
    pub fn with_m_cost(mut self, m_cost: u32) -> Self {
        self.m_cost = m_cost;
        self
    }

    /// 设置时间代价（迭代次数），支持链式调用。
    #[must_use]
    pub fn with_t_cost(mut self, t_cost: u32) -> Self {
        self.t_cost = t_cost;
        self
    }

    /// 设置并行度，支持链式调用。
    #[must_use]
    pub fn with_p_cost(mut self, p_cost: u32) -> Self {
        self.p_cost = p_cost;
        self
    }

    /// 设置输入 secret 的字节数上限，支持链式调用。
    #[must_use]
    pub fn with_max_secret_len(mut self, max_secret_len: usize) -> Self {
        self.max_secret_len = max_secret_len;
        self
    }

    /// 拒绝超长输入，挡住慢哈希 DoS。
    fn ensure_len(&self, secret: &[u8]) -> Result<()> {
        ensure!(
            secret.len() <= self.max_secret_len,
            SecretTooLongSnafu {
                len: secret.len(),
                max: self.max_secret_len,
            }
        );
        Ok(())
    }

    /// 按当前参数构造 Argon2id 实例。
    fn hasher(&self) -> Result<Argon2<'static>> {
        let params =
            Params::new(self.m_cost, self.t_cost, self.p_cost, None).context(InvalidParamsSnafu)?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }

    /// 用当前策略对 `secret` 加盐哈希，返回可直接入库的 PHC 字符串。
    ///
    /// 盐由 `hash_password` 内部经 getrandom 生成（password-hash 0.6 起的行为），
    /// 调用方不再需要、也不应该自己传 RNG——少一个能传错的参数。
    pub fn hash(&self, secret: &[u8]) -> Result<String> {
        self.ensure_len(secret)?;
        let hash: PasswordHash = self
            .hasher()?
            .hash_password(secret)
            .context(Argon2HashSnafu)?;
        Ok(hash.to_string())
    }

    /// 存储的哈希是否需要按当前策略重算。
    ///
    /// 判为「需要」的情形：旧式明文 sha256、无法解析的哈希、非 Argon2id 算法，
    /// 或代价参数弱于当前策略。
    ///
    /// **只升不降**：管理员调低参数时不会把存量哈希重算成更弱的版本——那是一次
    /// 静默的安全降级。确需统一参数请走离线迁移。
    pub fn needs_rehash(&self, stored: &str) -> bool {
        if is_legacy_sha256(stored) {
            return true;
        }
        let Ok(parsed) = PasswordHash::new(stored) else {
            // 解析不了的哈希本身就该被替换
            return true;
        };
        // argon2i / argon2d 用于口令存储都弱于 argon2id，一并升级
        // password-hash 0.6 起 Algorithm 只实现 TryFrom<&str>（不再有 TryFrom<Ident>）
        if !matches!(
            Algorithm::try_from(parsed.algorithm.as_str()),
            Ok(Algorithm::Argon2id)
        ) {
            return true;
        }
        let Ok(params) = Params::try_from(&parsed) else {
            return true;
        };
        // 只比两个真正的代价因子；p_cost 更高并不等于更强，不参与判定
        params.m_cost() < self.m_cost || params.t_cost() < self.t_cost
    }

    /// 校验 `secret` 是否匹配已存储的哈希 `stored`。
    ///
    /// - `stored` 为 Argon2 PHC 串：走标准 Argon2 验证（内部常数时间）。验证使用
    ///   **哈希串自带**的参数（`PasswordVerifier` 从 PHC 串读取），因此调高策略参数
    ///   后旧哈希依然验得过；只是会额外被标记为需要重算。
    /// - `stored` 为 64 位十六进制（旧式明文 sha256）：常数时间比对，命中返回
    ///   [`PasswordCheck::MatchedNeedsRehash`] 提示调用方升级。
    pub fn verify(&self, stored: &str, secret: &[u8]) -> Result<PasswordCheck> {
        // 长度检查放在最前：验证路径同样会跑一次 Argon2，DoS 面与 hash 一样大
        self.ensure_len(secret)?;

        // 旧式明文 sha256（无 `$` 前缀、恰好 64 位 hex）——常数时间比对，命中提示升级
        if is_legacy_sha256(stored) {
            return if constant_time_eq(stored.as_bytes(), secret) {
                Ok(PasswordCheck::MatchedNeedsRehash)
            } else {
                Ok(PasswordCheck::Mismatch)
            };
        }

        let parsed = PasswordHash::new(stored).context(PhcParseSnafu)?;
        // 用本策略构造的实例而非 `Argon2::default()`。
        //
        // 两者今天等价：`PasswordVerifier` 会从 PHC 串里读出算法 / 版本 / 代价参数
        // 重建 hasher，**只**保留实例上的 `secret`（pepper）。也正因为只保留 secret，
        // 一旦将来给策略加上 pepper（`Argon2::new_with_secret`），`default()` 会把它
        // 丢掉，于是所有验签静默失败——而这类故障在测试里极难发现（本地没配 pepper
        // 时行为完全正常）。这里始终走同一个构造入口，堵死这条路。
        match self.hasher()?.verify_password(secret, &parsed) {
            Ok(()) => {
                if self.needs_rehash(stored) {
                    Ok(PasswordCheck::MatchedNeedsRehash)
                } else {
                    Ok(PasswordCheck::Matched)
                }
            }
            // 仅「密码不匹配」归为 Mismatch；其余（哈希损坏 / 参数异常）作为服务端错误上抛
            // password-hash 0.6 起该变体由 Password 更名为 PasswordInvalid
            Err(argon2::password_hash::Error::PasswordInvalid) => Ok(PasswordCheck::Mismatch),
            Err(source) => Err(Error::Argon2Parse { source }),
        }
    }
}

/// 用默认策略对 `secret` 加盐哈希，返回可直接入库的 PHC 字符串。
///
/// `secret` 通常是客户端已 `sha256(password)` 处理的定长凭证——再套一层 Argon2 以获得
/// 加盐、慢哈希与 pass-the-hash 抵抗；即便直接传原始口令也同样适用。
///
/// 需要自定义代价参数时用 [`PasswordPolicy`]。
pub fn hash_password(secret: &[u8]) -> Result<String> {
    PasswordPolicy::default().hash(secret)
}

/// 用默认策略校验 `secret` 是否匹配已存储的哈希，见 [`PasswordPolicy::verify`]。
pub fn verify_password(stored: &str, secret: &[u8]) -> Result<PasswordCheck> {
    PasswordPolicy::default().verify(stored, secret)
}

/// 判断 `stored` 是否为旧式明文 sha256：恰好 64 位、全部十六进制字符。
/// Argon2 PHC 串以 `$argon2` 开头，天然不满足此条件。
fn is_legacy_sha256(stored: &str) -> bool {
    stored.len() == 64 && stored.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 常数时间比较，避免按字节短路造成的时序泄漏。
///
/// 走 `subtle` 而非手写 XOR 折叠：后者依赖「编译器不会把循环优化成短路比较」
/// 这一无法在源码层保证的假设。workspace 内 `tibba-totp` / `tibba-util` 同此。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
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

    /// 测试用的轻量策略：默认 19 MiB × 2 轮在单测里太慢，且本组用例
    /// 关心的是参数**比较**逻辑，不是哈希强度本身。
    fn cheap(m_cost: u32, t_cost: u32) -> PasswordPolicy {
        PasswordPolicy::new()
            .with_m_cost(m_cost)
            .with_t_cost(t_cost)
            .with_p_cost(1)
    }

    #[test]
    fn secret_longer_than_limit_is_rejected_on_both_paths() {
        let policy = cheap(8, 1).with_max_secret_len(16);
        let oversized = vec![b'x'; 17];

        // hash 路径
        assert!(matches!(
            policy.hash(&oversized).unwrap_err(),
            Error::SecretTooLong { len: 17, max: 16 }
        ));

        // verify 路径同样要挡——否则 DoS 只是从注册挪到了登录
        let stored = policy.hash(b"ok").unwrap();
        assert!(matches!(
            policy.verify(&stored, &oversized).unwrap_err(),
            Error::SecretTooLong { len: 17, max: 16 }
        ));

        // 恰好等于上限应放行
        assert!(policy.hash(&[b'x'; 16]).is_ok());
    }

    /// 参数升级后：旧哈希仍能验通过（参数取自 PHC 串），但被标记为需重算。
    #[test]
    fn stronger_policy_flags_old_hash_for_rehash_without_breaking_verify() {
        let weak = cheap(8, 1);
        let strong = cheap(16, 2);

        let stored = weak.hash(b"secret").unwrap();

        // 旧参数下：匹配且无需重算
        assert_eq!(
            weak.verify(&stored, b"secret").unwrap(),
            PasswordCheck::Matched
        );

        // 新参数下：**仍然验得过**，但提示重算
        assert_eq!(
            strong.verify(&stored, b"secret").unwrap(),
            PasswordCheck::MatchedNeedsRehash,
            "参数调高后旧哈希必须仍可验证，只是标记为待升级"
        );
        assert!(strong.needs_rehash(&stored));

        // 密码错了依然是 Mismatch，不会被 rehash 标记掩盖
        assert_eq!(
            strong.verify(&stored, b"wrong").unwrap(),
            PasswordCheck::Mismatch
        );

        // 用新策略重算后即不再需要升级
        let upgraded = strong.hash(b"secret").unwrap();
        assert!(!strong.needs_rehash(&upgraded));
        assert_eq!(
            strong.verify(&upgraded, b"secret").unwrap(),
            PasswordCheck::Matched
        );
    }

    /// 只升不降：调低参数不得把存量哈希标记为待重算（那是静默的安全降级）。
    #[test]
    fn weaker_policy_does_not_request_downgrade() {
        let strong = cheap(16, 2);
        let weak = cheap(8, 1);
        let stored = strong.hash(b"secret").unwrap();

        assert!(
            !weak.needs_rehash(&stored),
            "参数调低时不得把更强的存量哈希重算成更弱的版本"
        );
        assert_eq!(
            weak.verify(&stored, b"secret").unwrap(),
            PasswordCheck::Matched
        );
    }

    #[test]
    fn legacy_and_garbage_hashes_need_rehash() {
        let policy = cheap(8, 1);
        assert!(policy.needs_rehash(&"e".repeat(64)), "旧式明文 sha256");
        assert!(policy.needs_rehash("not-a-phc-string"), "无法解析的哈希");
        assert!(policy.needs_rehash(""), "空串");
    }

    /// **跨版本兼容守卫**：argon2 0.5 产出的哈希必须仍能验证通过。
    ///
    /// 这个 PHC 串是用 **argon2 0.5.3 + password-hash 0.5** 实际生成的，不是本
    /// 代码的产物。「本进程 hash 一次再 verify 一次」只能证明自洽，证明不了库
    /// 升级（argon2 0.5→0.6，PHC 解析换成 phc crate）之后**库里已有的**哈希
    /// 还认不认——而密码存储一旦认不回去，就是全量用户被锁在门外。
    ///
    /// 参数取的是 `PasswordPolicy::default()` 的组合（m=19456, t=2, p=1），
    /// 即线上存量哈希的实际形态。
    #[test]
    fn hash_from_argon2_0_5_still_verifies() {
        let stored = "$argon2id$v=19$m=19456,t=2,p=1$0e2N3DNvIUvEizt0BfRMjw$\
                      bz8bPlMbKr3+1LVhk/YRVKYTPu9GGZD9okPSWzV4Iis";
        let secret = b"correct horse battery staple";

        assert_eq!(
            verify_password(stored, secret).unwrap(),
            PasswordCheck::Matched,
            "argon2 0.5 生成的哈希必须仍然验得过"
        );
        assert_eq!(
            verify_password(stored, b"wrong").unwrap(),
            PasswordCheck::Mismatch
        );
        // 参数与当前默认策略一致，不应被判为需要重算
        assert!(!PasswordPolicy::default().needs_rehash(stored));
    }

    /// 非 argon2id（argon2i/argon2d）用于口令存储更弱，应被标记升级。
    #[test]
    fn non_argon2id_algorithm_needs_rehash() {
        let params = Params::new(8, 1, 1, None).unwrap();
        let argon2i = Argon2::new(Algorithm::Argon2i, Version::V0x13, params);
        // 盐由 hash_password 内部生成（password-hash 0.6 起不再接收 RNG 参数）
        let stored: PasswordHash = argon2i.hash_password(b"secret").unwrap();
        let stored = stored.to_string();
        assert!(stored.starts_with("$argon2i$"));

        let policy = cheap(8, 1);
        assert!(policy.needs_rehash(&stored), "argon2i 应被升级为 argon2id");
        // 但仍然验得过，用户不会被锁在门外
        assert_eq!(
            policy.verify(&stored, b"secret").unwrap(),
            PasswordCheck::MatchedNeedsRehash
        );
    }

    #[test]
    fn invalid_params_are_reported_not_panicked() {
        // m_cost 必须 ≥ 8×p_cost，这里故意违反
        let bad = PasswordPolicy::new().with_m_cost(1).with_p_cost(4);
        assert!(matches!(
            bad.hash(b"x").unwrap_err(),
            Error::InvalidParams { .. }
        ));
    }

    /// 默认策略的参数即 argon2 crate 的推荐值。
    #[test]
    fn default_policy_matches_argon2_recommended_params() {
        let policy = PasswordPolicy::default();
        assert_eq!(policy.m_cost, Params::DEFAULT_M_COST);
        assert_eq!(policy.t_cost, Params::DEFAULT_T_COST);
        assert_eq!(policy.p_cost, Params::DEFAULT_P_COST);
        assert_eq!(policy.max_secret_len, DEFAULT_MAX_SECRET_LEN);
    }
}
