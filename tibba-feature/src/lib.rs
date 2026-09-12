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
//! ## 进程内缓存（可选）
//! 默认**每次** [`FeatureFlags::is_enabled`] 都读一次 Redis。开关是典型的
//! 「几乎不变、每个请求都要看」的数据，用 [`FeatureFlags::with_local_cache`]
//! 挂上一层进程内缓存即可把这条往返省掉，代价是开关变更最多延迟一个 TTL
//! 才在各节点生效。
//!
//! 默认不开启是有意的：本模块的用途包含「线上急停」，而急停的生效速度应当由
//! 部署方按自己的 SLA 决定，不该由库替他做主。
//!
//! ## 故障默认安全
//! [`FeatureFlags::is_enabled`] 在读失败 / 开关缺失时一律返回 `false`——宁可不开新
//! 特性，也不在 Redis 抖动时误放量。
//!
//! ## 一致性
//! [`FeatureFlags::set`] / [`FeatureFlags::remove`] 是「读-改-写」，非原子。开关由
//! 管理员低频改动，冲突概率极低；如需强一致可在调用侧加分布式锁。
//!
//! 读-改-写的「读」**始终直连 Redis**，不走进程内缓存——否则两个节点可能各自
//! 基于自己的陈旧快照改写，后写的那个会把对方的改动整片抹掉。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::time::Duration;
use tibba_cache::{RedisCache, TwoLevelStore};
use tibba_error::Error as BaseError;

/// 存放所有开关的 Redis 键。
const FLAGS_KEY: &str = "feature_flags";

/// 写入 TTL：约 10 年，近似「永不过期」（避免新增持久化 API）。
const PERSIST_TTL: Duration = Duration::from_secs(10 * 365 * 24 * 60 * 60);

/// 进程内缓存只存 [`FLAGS_KEY`] 一个键。
const LOCAL_CACHE_SIZE: NonZeroUsize = NonZeroUsize::new(1).expect("1 不是 0");

type Result<T> = std::result::Result<T, BaseError>;

/// 全部开关的内存表示。
type Flags = BTreeMap<String, bool>;

/// 单个开关的展示结构（管理端点列表用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    /// 开关名（如 `new_dashboard`）
    pub name: String,
    /// 是否开启
    pub enabled: bool,
}

/// 特性开关服务。
///
/// 默认无内部状态，可自由 Clone；调用 [`Self::with_local_cache`] 后会额外持有
/// 一层进程内缓存。
pub struct FeatureFlags {
    /// 权威存储。读-改-写与未开启本地缓存时的读取都走这里。
    cache: &'static RedisCache,
    /// 可选的进程内缓存层（L1 进程内 + L2 Redis）。
    ///
    /// L1 的 TTL 对齐到墙钟边界，使集群内各节点在**同一秒**回源刷新——
    /// 开关的生效时刻因此在全集群一致，而不是各节点按自己的写入时刻错开。
    local: Option<TwoLevelStore<Flags>>,
}

impl Clone for FeatureFlags {
    /// 克隆只复制权威引用，**不**复制进程内缓存。
    ///
    /// `TwoLevelStore` 持有互斥锁保护的 LRU，不可 Clone；更重要的是，复制一份
    /// 独立的 L1 会让同一进程内出现两份可能不一致的快照。需要共享缓存的场景
    /// 应当共享同一个 `FeatureFlags`（如放进 `OnceLock` / `&'static`）。
    fn clone(&self) -> Self {
        Self {
            cache: self.cache,
            local: None,
        }
    }
}

impl FeatureFlags {
    /// 以给定的 RedisCache 创建服务，每次读取都直连 Redis。
    pub fn new(cache: &'static RedisCache) -> Self {
        Self {
            cache,
            local: None,
        }
    }

    /// 挂上进程内缓存，`ttl` 为本地快照的刷新周期，支持链式调用。
    ///
    /// 开启后 [`Self::is_enabled`] 在 `ttl` 内直接读内存，不再访问 Redis。
    ///
    /// # 代价
    /// 开关的变更最多延迟一个 `ttl` 才在**其它**节点生效（本节点的写入会立刻
    /// 更新自己的 L1）。用于「线上急停」时，`ttl` 就是急停生效时间的上界，
    /// 按自己的 SLA 取值——几秒通常是个合理起点。
    ///
    /// Redis 侧的存储时长不受影响，仍是 [`PERSIST_TTL`]（近似永久）。
    #[must_use]
    pub fn with_local_cache(mut self, ttl: Duration) -> Self {
        self.local = Some(
            TwoLevelStore::new(self.cache.clone(), LOCAL_CACHE_SIZE, ttl)
                // 开关在 Redis 里近似永不过期，不能跟着 L1 的刷新周期一起失效
                .with_l2_ttl(PERSIST_TTL)
                // 只有一个键，没有「大量 key 同时过期」可打散，抖动无意义
                .with_jitter_percent(0),
        );
        self
    }

    /// 读取全部开关，**始终直连 Redis**，绕过进程内缓存。
    ///
    /// 读-改-写路径必须用它：若基于陈旧的本地快照改写，另一个节点在此期间的
    /// 改动会被整片覆盖掉。
    async fn load_authoritative(&self) -> Result<Flags> {
        Ok(self.cache.get_struct::<Flags>(FLAGS_KEY).await?.unwrap_or_default())
    }

    /// 读取全部开关，开启进程内缓存时优先走缓存。
    async fn load(&self) -> Result<Flags> {
        match &self.local {
            Some(store) => Ok(store.get(FLAGS_KEY).await?.unwrap_or_default()),
            None => self.load_authoritative().await,
        }
    }

    /// 覆盖写回全部开关；开启进程内缓存时一并刷新本节点的 L1。
    async fn store(&self, flags: &Flags) -> Result<()> {
        match &self.local {
            Some(store) => store.set(FLAGS_KEY, flags.clone()).await?,
            None => {
                self.cache
                    .set_struct(FLAGS_KEY, flags, Some(PERSIST_TTL))
                    .await?
            }
        }
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
        let mut flags = self.load_authoritative().await?;
        flags.insert(name.into(), enabled);
        self.store(&flags).await
    }

    /// 删除某开关。返回 `true` 表示原本存在并已删除。
    pub async fn remove(&self, name: &str) -> Result<bool> {
        let mut flags = self.load_authoritative().await?;
        let existed = flags.remove(name).is_some();
        if existed {
            self.store(&flags).await?;
        }
        Ok(existed)
    }

    /// 列出全部开关，按名称有序（`BTreeMap` 天然有序）。
    ///
    /// 走权威读取：管理端列表应当反映 Redis 的当前状态，而不是本节点的快照。
    pub async fn list(&self) -> Result<Vec<FeatureFlag>> {
        let flags = self.load_authoritative().await?;
        Ok(flags
            .into_iter()
            .map(|(name, enabled)| FeatureFlag { name, enabled })
            .collect())
    }
}
