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

//! Redis 持久化的特性开关（Feature Flags）。
//!
//! 适合「灰度发布 / 线上急停」这类**低频管理、高频读取**的布尔开关，让功能的
//! 开关无需改代码 / 重启即可生效。
//!
//! ## 存储
//! 全部开关存于单个 Redis 键 [`FLAGS_KEY`] 下的 JSON 对象（`name → bool`），读写都
//! 复用现成的 [`RedisCache`]。写入用一个很长的 TTL 近似「永不过期」，避免引入新的
//! 持久化 API。
//!
//! ## 故障默认安全
//! [`FeatureFlags::is_enabled`] 在读失败 / 开关缺失时一律返回 `false`——宁可不开新
//! 特性，也不在 Redis 抖动时误放量。
//!
//! ## 一致性
//! [`FeatureFlags::set`] / [`FeatureFlags::remove`] 是「读-改-写」，非原子。开关由
//! 管理员低频改动，冲突概率极低；如需强一致可在调用侧加分布式锁。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::Error as BaseError;

/// 存放所有开关的 Redis 键。
const FLAGS_KEY: &str = "feature_flags";

/// 写入 TTL：约 10 年，近似「永不过期」（避免新增持久化 API）。
const PERSIST_TTL: Duration = Duration::from_secs(10 * 365 * 24 * 60 * 60);

type Result<T> = std::result::Result<T, BaseError>;

/// 单个开关的展示结构（管理端点列表用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    /// 开关名（如 `new_dashboard`）
    pub name: String,
    /// 是否开启
    pub enabled: bool,
}

/// 特性开关服务。仅持有 [`RedisCache`] 引用，无内部状态，可自由 Clone。
#[derive(Clone)]
pub struct FeatureFlags {
    cache: &'static RedisCache,
}

impl FeatureFlags {
    /// 以给定的 RedisCache 创建服务。
    pub fn new(cache: &'static RedisCache) -> Self {
        Self { cache }
    }

    /// 读取全部开关（`name → enabled`）。键不存在时返回空表。
    async fn load(&self) -> Result<BTreeMap<String, bool>> {
        Ok(self
            .cache
            .get_struct::<BTreeMap<String, bool>>(FLAGS_KEY)
            .await?
            .unwrap_or_default())
    }

    /// 覆盖写回全部开关。
    async fn store(&self, flags: &BTreeMap<String, bool>) -> Result<()> {
        self.cache
            .set_struct(FLAGS_KEY, flags, Some(PERSIST_TTL))
            .await?;
        Ok(())
    }

    /// 判断某开关是否开启。**故障默认安全**：读失败或开关不存在均返回 `false`。
    pub async fn is_enabled(&self, name: &str) -> bool {
        self.load()
            .await
            .ok()
            .and_then(|flags| flags.get(name).copied())
            .unwrap_or(false)
    }

    /// 设置（新增 / 覆盖）某开关的开关态。
    pub async fn set(&self, name: impl Into<String>, enabled: bool) -> Result<()> {
        let mut flags = self.load().await?;
        flags.insert(name.into(), enabled);
        self.store(&flags).await
    }

    /// 删除某开关。返回 `true` 表示原本存在并已删除。
    pub async fn remove(&self, name: &str) -> Result<bool> {
        let mut flags = self.load().await?;
        let existed = flags.remove(name).is_some();
        if existed {
            self.store(&flags).await?;
        }
        Ok(existed)
    }

    /// 列出全部开关，按名称有序（`BTreeMap` 天然有序）。
    pub async fn list(&self) -> Result<Vec<FeatureFlag>> {
        let flags = self.load().await?;
        Ok(flags
            .into_iter()
            .map(|(name, enabled)| FeatureFlag { name, enabled })
            .collect())
    }
}
