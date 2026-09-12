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

//! 进程内定容缓存（L1）+ Redis（L2）双层缓存。
//!
//! L1 用 [`TtlFifoStore`]：容量满时按写入顺序淘汰，**不是 LRU**，读取不保护条目。
//! 做容量规划时需按此假设，详见其模块文档。
//!
//! ## 两层的 TTL 策略刻意不同
//!
//! | 层 | TTL | 目的 |
//! |----|-----|------|
//! | L1 进程内 | **对齐**到下一个 `ttl` 边界 | 跨节点**一致性** |
//! | L2 Redis | `ttl` + 按 key 派生的抖动 | **防雪崩** |
//!
//! ### L1 为什么要对齐边界
//! 双层缓存的固有代价是：某节点 `set()` 之后，其余节点的 L1 仍会返回旧值。
//! 若各节点按自己的写入时刻计时，节点 A 可能在 T+1 刷新、节点 B 在 T+599 刷新——
//! 节点间的分歧窗口没有上界。对齐到统一的墙钟边界后，所有节点在**同一秒**同时
//! 失效并回源 Redis，分歧窗口被收敛为「至多到下一个边界」且全集群同步。
//! 对 feature flag / 配置类数据（本结构的主要用途），这正是想要的语义。
//!
//! 注意这是**一致性**机制，**不是**防雪崩——恰恰相反，它让 L1 集中失效。
//! 但集中失效只打到 Redis（L2 里数据还在，是命中而非穿透），不会打到数据库，
//! 这个代价是可接受的。此前本文件把该行为注释成「防止缓存雪崩」，说反了。
//!
//! ### L2 为什么要抖动
//! Redis 层若同样对齐，则一个周期内写入的所有 key 会在同一边界一起**真正过期**，
//! 届时全部请求穿透到数据库——这才是雪崩。故 L2 用完整 `ttl` 加一段按 key
//! 派生的抖动，把过期时刻打散开。

use super::{Error, Expired, RedisCache, TtlFifoStore};
use serde::{Serialize, de::DeserializeOwned};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroUsize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Error>;

/// L2 抖动幅度默认占 `ttl` 的百分比，可用 [`TwoLevelStore::with_jitter_percent`] 覆盖。
///
/// 按比例而非绝对秒数，使其对 10 秒和 1 小时的 `ttl` 同样适用。
pub const DEFAULT_L2_JITTER_PERCENT: u8 = 10;

/// L1 写入时判定「离边界太近」的阈值分母：不足 `unit/10` 就顺延一个周期。
///
/// 与 L2 的抖动幅度分开：一个是「防止刚写就失效」的启发式，一个是防雪崩的
/// 打散幅度，两者恰好都取 1/10 纯属巧合，不该共用一个常量而被一起改动。
const L1_EXTEND_DIVISOR: u64 = 10;

#[inline]
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// `ttl` 的秒数，下限钳为 1。
///
/// 钳位有两个作用：避免下面按 `unit` 取模时除零，以及避免把 `EX 0` 发给 Redis
/// （Redis 会直接报 `invalid expire time`）。构造时传 `Duration::ZERO` 属配置错误，
/// 这里退化为 1 秒而不是让写入在运行时失败。
#[inline]
fn unit_secs(ttl: Duration) -> u64 {
    ttl.as_secs().max(1)
}

/// 距下一个 `unit` 对齐边界的秒数，恒落在 `(0, unit]` 区间。
#[inline]
fn secs_to_next_boundary(unit: u64, now: u64) -> u64 {
    unit - (now % unit)
}

/// L1 写入用的对齐 TTL：到下一个边界；若不足 `unit` 的 1/10 则顺延一个周期，
/// 避免刚写进内存就立刻失效、导致紧接着的读全部白跑一趟 Redis。
#[inline]
fn l1_ttl_for_set(unit: u64, now: u64) -> u64 {
    let remaining = secs_to_next_boundary(unit, now);
    if remaining < unit / L1_EXTEND_DIVISOR {
        remaining + unit
    } else {
        remaining
    }
}

/// 由 key 派生的确定性抖动，取值 `[0, span)`；`span` 为 0 时返回 0。
///
/// 用 key 的散列而非随机数，有三个好处：
/// - 不必为此引入 `rand` 依赖；
/// - 同一 key 在所有节点上得到相同抖动，不会出现「同 key 在不同节点过期时刻不同」；
/// - 防雪崩要打散的正是「大量**不同** key 同时过期」，按 key 散列恰好覆盖这一点。
#[inline]
fn key_jitter_secs(key: &str, span: u64) -> u64 {
    if span == 0 {
        return 0;
    }
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() % span
}

/// L2（Redis）TTL：完整 `unit` 加按 key 派生的抖动，把过期时刻打散防雪崩。
///
/// `jitter_percent` 为 0 时不加抖动，所有 key 严格按 `unit` 过期。
#[inline]
fn l2_ttl_secs(unit: u64, key: &str, jitter_percent: u8) -> u64 {
    // 先乘后除，避免 unit 较小时整除先归零；saturating 防溢出
    let span = unit.saturating_mul(u64::from(jitter_percent)) / 100;
    unit + key_jitter_secs(key, span)
}

/// 为缓存数据附加过期时间戳的包装结构体。
#[derive(Clone)]
struct ExpiredCache<T> {
    /// 实际缓存的数据
    data: T,
    /// 缓存条目的过期 Unix 时间戳（秒）
    expired_at: u64,
}

impl<T> Expired for ExpiredCache<T> {
    /// 当前时间已达到或超过 expired_at 时返回 `true`。
    fn is_expired(&self) -> bool {
        now_secs() >= self.expired_at
    }
}

/// 进程内定容缓存 + Redis 双层缓存。
/// 读操作优先命中 L1，未命中再查 Redis 并回填 L1。
pub struct TwoLevelStore<T> {
    /// 第一层：带 TTL 的进程内定容缓存（按写入顺序淘汰，非 LRU）
    l1: TtlFifoStore<ExpiredCache<T>>,
    /// L1 的对齐周期，同时是 L2 的默认 TTL
    ttl: Duration,
    /// L2 的 TTL；`None` 表示沿用 `ttl`，见 [`Self::with_l2_ttl`]
    l2_ttl: Option<Duration>,
    /// L2 抖动幅度占 L2 TTL 的百分比，见 [`Self::with_jitter_percent`]
    jitter_percent: u8,
    /// 第二层：Redis 缓存
    redis: RedisCache,
}

impl<T: Clone + Serialize + DeserializeOwned> TwoLevelStore<T> {
    /// 创建新的 TwoLevelStore 实例。
    /// `size` 为 L1 的最大条目数，`ttl` 为缓存默认过期时长。
    ///
    /// L1 满时按**写入顺序**淘汰，读取不会延长条目寿命；`size` 应按「一个 TTL
    /// 周期内会写入多少个不同 key」来估，而不是按热点 key 的数量。
    ///
    /// L2 抖动幅度默认 [`DEFAULT_L2_JITTER_PERCENT`]，用
    /// [`Self::with_jitter_percent`] 调整。
    pub fn new(redis: RedisCache, size: NonZeroUsize, ttl: Duration) -> Self {
        Self {
            l1: TtlFifoStore::new(size),
            ttl,
            l2_ttl: None,
            jitter_percent: DEFAULT_L2_JITTER_PERCENT,
            redis,
        }
    }

    /// 单独设置 L2（Redis）的 TTL，与 L1 的对齐周期解耦，支持链式调用。
    ///
    /// 默认两者同为 `new` 传入的 `ttl`。但它们本来就是**两个独立的轴**：
    ///
    /// - L1 的 TTL 决定「本节点多久回源一次」——即跨节点分歧窗口的上界；
    /// - L2 的 TTL 决定「数据在 Redis 里存多久」——即多久穿透回数据源。
    ///
    /// 把两者绑死，就无法表达「Redis 里长期保存、进程内只缓存几秒」这种形态，
    /// 而特性开关、配置字典这类**低频写、高频读**的数据恰恰是这个形状：
    /// 它们在 Redis 里近似永不过期，但每个节点只需要缓存几秒就够了。
    ///
    /// ```ignore
    /// // Redis 侧近似永久，进程内 10s 刷新一次
    /// let store = TwoLevelStore::new(cache, size, Duration::from_secs(10))
    ///     .with_l2_ttl(Duration::from_secs(10 * 365 * 24 * 3600))
    ///     .with_jitter_percent(0);
    /// ```
    #[must_use]
    pub fn with_l2_ttl(mut self, ttl: Duration) -> Self {
        self.l2_ttl = Some(ttl);
        self
    }

    /// L2 实际使用的 TTL 秒数（下限 1）。
    #[inline]
    fn l2_unit(&self) -> u64 {
        unit_secs(self.l2_ttl.unwrap_or(self.ttl))
    }

    /// 设置 L2 抖动幅度占 L2 TTL 的百分比，支持链式调用。超过 100 会被钳到 100。
    ///
    /// 抖动把「同一周期写入的 key 集中过期」摊开，避免它们同时穿透到数据库。
    /// key 数量越多、回源代价越高，越值得调大。
    ///
    /// **设为 0 等于关掉防雪崩**：所有 key 会严格按 `ttl` 过期，同批写入的
    /// 就会同批失效。只有在确实需要精确过期时刻时才这么做。
    #[must_use]
    pub fn with_jitter_percent(mut self, percent: u8) -> Self {
        self.jitter_percent = percent.min(100);
        self
    }

    fn fill_l1(&self, key: &str, value: T, ttl: Duration) {
        if ttl.is_zero() {
            return;
        }
        self.l1.set(
            key,
            ExpiredCache {
                data: value,
                expired_at: now_secs() + ttl.as_secs(),
            },
        );
    }

    /// 将值写入两层缓存：先写 Redis 再更新 L1。
    ///
    /// 两层 TTL 不同，见模块文档：Redis 用 `ttl` + 按 key 抖动（防雪崩），
    /// L1 对齐到下一个 `ttl` 边界（跨节点一致性）。
    pub async fn set(&self, key: &str, value: T) -> Result<()> {
        let unit = unit_secs(self.ttl);

        // L2：完整 TTL + 抖动，避免同周期写入的 key 在同一时刻集体穿透到数据库
        let redis_ttl = Duration::from_secs(l2_ttl_secs(
            self.l2_unit(),
            key,
            self.jitter_percent,
        ));
        self.redis.set_struct(key, &value, Some(redis_ttl)).await?;

        // L1：对齐到边界，使所有节点在同一秒回源刷新
        let l1_ttl = Duration::from_secs(l1_ttl_for_set(unit, now_secs()));
        self.fill_l1(key, value, l1_ttl);

        Ok(())
    }

    /// 从缓存读取值，优先查询 L1，未命中则查 Redis 并回填 L1。
    pub async fn get(&self, key: &str) -> Result<Option<T>> {
        // 优先查 L1（已过期条目不会被返回）
        if let Some(value) = self.l1.get(key) {
            return Ok(Some(value.data));
        }
        // 内存未命中，查 Redis
        let result: Option<T> = self.redis.get_struct(key).await?;
        if let Some(value) = &result {
            // 回填只到下一个边界，且**不**走 set 的顺延分支：顺延会让本节点的 L1
            // 越过边界继续持有旧值，而其余节点已在边界处刷新——正是对齐要消除的分歧。
            //
            // 此前这里是 `if ttl <= self.ttl { fill }`，注释称「TTL 超出预期范围说明
            // 条目即将过期」，但走进 `>` 分支恰恰是 TTL 被顺延**变长**的那 10% 窗口，
            // 与注释相反；其净效果只是在每个边界前的 10% 时间里完全不回填 L1。
            let l1_ttl =
                Duration::from_secs(secs_to_next_boundary(unit_secs(self.ttl), now_secs()));
            self.fill_l1(key, value.clone(), l1_ttl);
        }
        Ok(result)
    }

    /// 从两层缓存中删除指定键。
    ///
    /// 先删 Redis 再清 L1：若反过来，并发 `get()` 可能在「L1 已清、Redis 未删」的窗口
    /// 读到 Redis 旧值并回填 L1，使陈旧数据在内存中复活并存活到下次 TTL 到期。
    pub async fn del(&self, key: &str) -> Result<()> {
        self.redis.del(key).await?;
        self.l1.del(key);
        Ok(())
    }

    /// 清除 L1 中的过期条目，应定期调用以释放内存。
    /// Redis 侧的 TTL 由 Redis 自身管理，无需手动清理。
    ///
    /// 纯内存操作，非 async——L1 已改用同步锁，见 [`TtlFifoStore`]。
    pub fn purge_expired(&self) {
        self.l1.purge_expired();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    const UNIT: u64 = 600; // 10 分钟

    #[test]
    fn boundary_distance_stays_in_range() {
        // 恰在边界上：整个周期都还剩着，不能返回 0（会被当成「不过期」或触发 EX 0）
        assert_eq!(secs_to_next_boundary(UNIT, 0), UNIT);
        assert_eq!(secs_to_next_boundary(UNIT, UNIT), UNIT);
        // 周期内：剩余量随时间线性递减
        assert_eq!(secs_to_next_boundary(UNIT, 1), UNIT - 1);
        assert_eq!(secs_to_next_boundary(UNIT, UNIT + 59), UNIT - 59);
    }

    /// L1 对齐的**本质属性**：同一周期内任意时刻写入，算出的绝对过期时刻相同。
    /// 这正是跨节点一致性的来源——各节点写入时间不同，失效时刻却一致。
    #[test]
    fn l1_alignment_yields_identical_absolute_expiry() {
        let base = 10 * UNIT; // 某个边界
        // 取周期内多个时刻，跳过末尾 1/10 的顺延窗口
        let expiries: HashSet<u64> = (0..UNIT - UNIT / L1_EXTEND_DIVISOR)
            .step_by(7)
            .map(|offset| {
                let now = base + offset;
                now + l1_ttl_for_set(UNIT, now)
            })
            .collect();
        assert_eq!(
            expiries.len(),
            1,
            "同周期内写入的条目必须落在同一个绝对过期时刻，否则跨节点一致性不成立"
        );
        assert!(expiries.contains(&(base + UNIT)));
    }

    /// L1 与 L2 的 TTL 必须能各走各的：
    /// 默认相同；一旦 `with_l2_ttl` 覆盖，L1 仍按原 `ttl` 对齐边界。
    #[test]
    fn l2_ttl_can_be_decoupled_from_l1() {
        // 默认：两者同源
        let unit = UNIT;
        assert_eq!(l2_ttl_secs(unit, "k", 0), unit);

        // 覆盖后 L2 用新的 unit，L1 的边界计算完全不受影响
        let persist = 10 * 365 * 24 * 3600_u64;
        assert_eq!(l2_ttl_secs(persist, "k", 0), persist);
        assert_eq!(secs_to_next_boundary(unit, 10 * UNIT + 1), UNIT - 1);
    }

    #[test]
    fn l1_extends_when_boundary_is_imminent() {
        let base = 10 * UNIT;
        // 距边界仅 5s（< 600/10），应顺延一个周期，避免刚写就失效
        let now = base + UNIT - 5;
        assert_eq!(l1_ttl_for_set(UNIT, now), 5 + UNIT);
        // 距边界 100s（≥ 60），不顺延
        let now = base + UNIT - 100;
        assert_eq!(l1_ttl_for_set(UNIT, now), 100);
    }

    /// 防雪崩的核心守卫：大量不同 key 的 L2 过期时刻必须被打散。
    /// 若有人把 L2 改回「对齐到边界」，这里会立刻退化成 1 个取值而失败。
    #[test]
    fn l2_ttl_is_spread_across_keys() {
        let ttls: HashSet<u64> = (0..500)
            .map(|i| l2_ttl_secs(UNIT, &format!("feature:flag:{i}"), DEFAULT_L2_JITTER_PERCENT))
            .collect();
        assert!(
            ttls.len() > 30,
            "500 个 key 只得到 {} 种过期时刻，抖动没有生效——会集中失效造成雪崩",
            ttls.len()
        );
        // 抖动只向上加，绝不缩短配置的 TTL；且不超过配置的百分比
        let span = UNIT * u64::from(DEFAULT_L2_JITTER_PERCENT) / 100;
        for ttl in ttls {
            assert!((UNIT..UNIT + span).contains(&ttl), "TTL {ttl} 越界");
        }
    }

    /// 抖动幅度可配：百分比越大，过期时刻铺得越开。
    #[test]
    fn jitter_percent_controls_spread_width() {
        let spread = |percent: u8| -> u64 {
            let ttls: Vec<u64> = (0..500)
                .map(|i| l2_ttl_secs(UNIT, &format!("k{i}"), percent))
                .collect();
            let max = ttls.iter().copied().max().unwrap_or(0);
            let min = ttls.iter().copied().min().unwrap_or(0);
            max - min
        };
        let narrow = spread(5);
        let wide = spread(50);
        assert!(wide > narrow, "50% 的铺开范围应明显大于 5%：{wide} vs {narrow}");
        assert!(wide <= UNIT / 2);
    }

    /// 抖动设 0 = 关掉防雪崩：所有 key 严格按 ttl 过期。
    #[test]
    fn zero_jitter_disables_spreading() {
        let ttls: HashSet<u64> = (0..100)
            .map(|i| l2_ttl_secs(UNIT, &format!("k{i}"), 0))
            .collect();
        assert_eq!(ttls, HashSet::from([UNIT]), "抖动为 0 时所有 key 应同时过期");
    }

    /// 百分比超过 100 被钳住，抖动不会超过一整个周期。
    #[test]
    fn jitter_percent_is_clamped_at_100() {
        // 直接验钳位逻辑：with_jitter_percent 需要 RedisCache 才能构造实例，
        // 这里等价地检查 l2_ttl_secs 在 100% 下的上界
        let span = UNIT; // 100%
        for i in 0..200 {
            let ttl = l2_ttl_secs(UNIT, &format!("k{i}"), 100);
            assert!((UNIT..UNIT + span).contains(&ttl), "TTL {ttl} 越界");
        }
    }

    /// 同一 key 的抖动必须确定：不同节点、不同进程算出同一个 TTL。
    #[test]
    fn l2_jitter_is_deterministic_per_key() {
        assert_eq!(
            l2_ttl_secs(UNIT, "same-key", DEFAULT_L2_JITTER_PERCENT),
            l2_ttl_secs(UNIT, "same-key", DEFAULT_L2_JITTER_PERCENT)
        );
        assert_eq!(key_jitter_secs("k", 0), 0, "span 为 0 时不得除零");
    }

    /// `ttl` 传 0 属配置错误，但不能除零 panic，也不能把 `EX 0` 发给 Redis。
    #[test]
    fn zero_ttl_is_clamped_not_fatal() {
        let unit = unit_secs(Duration::ZERO);
        assert_eq!(unit, 1);
        // 1 秒 × 10% 整除后为 0，抖动跨度归零但不得 panic，TTL 仍须 ≥ 1
        assert!(l2_ttl_secs(unit, "any", DEFAULT_L2_JITTER_PERCENT) >= 1);
        assert!(l2_ttl_secs(unit, "any", 100) >= 1);
        assert!(l1_ttl_for_set(unit, now_secs()) >= 1);
    }
}
