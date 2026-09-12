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

//! 带 TTL 的定容内存缓存，容量满时**按写入顺序**淘汰。
//!
//! ## 为什么叫 FIFO 而不是 LRU
//! 本结构以 `lru::LruCache` 为底层容器，但读路径只用 `peek`（不更新访问时序），
//! 于是「最近最少使用」的次序退化为「最早写入」的次序——淘汰的是最老的条目，
//! 与读取热度无关。热点 key 若长期不被重写，容量满时同样会被淘汰。
//!
//! 这是刻意的取舍：`peek` 不改动容器状态，读路径的临界区因此短得多。代价是
//! 失去 LRU 的热点保护。
//!
//! 本类型此前叫 `TtlLruStore`，名字与实际淘汰策略不符，已更名以免调用方按 LRU
//! 的热度假设做容量规划。若确需 LRU 语义，应改用 `LruCache::get` 并接受它更新
//! 时序带来的开销，而不是沿用本类型。

use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Mutex, MutexGuard};

/// 用于判断缓存数据是否已过期的 trait。
pub trait Expired {
    /// 返回 `true` 表示数据已过期，应从缓存中移除。
    fn is_expired(&self) -> bool;
}

/// 线程安全的 TTL + 定容 FIFO 缓存。
///
/// 两级失效：条目自身的 TTL（[`Expired`]）+ 容量满时按写入顺序淘汰。
/// 淘汰语义见模块文档——**不是 LRU**。
///
/// ## 为什么用同步 `Mutex` 而非 `tokio::sync::RwLock`
/// 所有操作都是纯内存的哈希查找 / 插入，临界区内**没有任何 await**。async 锁的
/// 意义在于「持锁跨越 await 点时不占住 worker 线程」，这里用不上，只会白付
/// 一次任务挂起/唤醒的调度开销。
///
/// 选 `Mutex` 而非 `std::sync::RwLock`：读路径的临界区只有一次哈希查找加一次
/// clone，短到读写锁的额外簿记开销盖过并发收益。
pub struct TtlFifoStore<T> {
    cache: Mutex<LruCache<String, T>>,
}

impl<T: Expired + Clone> TtlFifoStore<T> {
    /// 创建指定容量的 `TtlFifoStore`，容量必须大于 0。
    pub fn new(size: NonZeroUsize) -> Self {
        Self {
            cache: Mutex::new(LruCache::new(size)),
        }
    }

    /// 取锁。中毒时取回内部数据继续用：本结构只是缓存，
    /// 一个 panic 的持锁者最多留下一条半写的条目，不值得让整个进程不可用。
    fn lock(&self) -> MutexGuard<'_, LruCache<String, T>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 向缓存写入键值对。容量已满时淘汰**最早写入**的条目。
    ///
    /// 重复写入同一 key 会把它移到队尾（刷新其在淘汰序列中的位置），
    /// 故准确说是按「最后一次写入」排序，而非按首次插入。
    pub fn set(&self, key: &str, value: T) {
        self.lock().put(key.to_string(), value);
    }

    /// 读取未过期的缓存值，键不存在或已过期时返回 `None`。
    ///
    /// 走 `peek` 而非 `get`：不更新访问时序。这正是本结构退化为 FIFO 的原因，
    /// 见模块文档。
    ///
    /// 命中过期条目时**顺手删除**它。此前只是「读不到」，条目仍占着容量——
    /// 一个装满过期条目的 store 会在下次写入时把**活着**的条目淘汰掉，
    /// 而清理全靠调用方记得定期调 [`Self::purge_expired`]（实际上没人调）。
    /// 顺带清理让容量语义回到「最多 N 个**有效**条目」这个直觉上。
    pub fn get(&self, key: &str) -> Option<T> {
        let mut cache = self.lock();
        if let Some(value) = cache.peek(key) {
            if !value.is_expired() {
                return Some(value.clone());
            }
            cache.pop(key);
        }
        None
    }

    /// 删除指定键，键不存在时为空操作。
    pub fn del(&self, key: &str) {
        self.lock().pop(key);
    }

    /// 一次性清除所有已过期条目。
    ///
    /// [`Self::get`] 已经会顺手删除读到的过期条目，因此本方法**不是**正确性
    /// 的必要条件；它用于「从此不再被读到的键」——那些条目只能靠容量淘汰
    /// 或这里的批量清理来释放。
    pub fn purge_expired(&self) {
        let mut cache = self.lock();
        // LruCache 不支持迭代中删除，需先收集过期键再批量移除
        let keys: Vec<String> = cache
            .iter()
            .filter(|(_, v)| v.is_expired())
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            cache.pop(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// 过期与否直接由字段决定，避免测试依赖真实时钟。
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Entry {
        tag: &'static str,
        expired: bool,
    }

    impl Entry {
        fn live(tag: &'static str) -> Self {
            Self { tag, expired: false }
        }
        fn dead(tag: &'static str) -> Self {
            Self { tag, expired: true }
        }
    }

    impl Expired for Entry {
        fn is_expired(&self) -> bool {
            self.expired
        }
    }

    fn store(cap: usize) -> TtlFifoStore<Entry> {
        TtlFifoStore::new(NonZeroUsize::new(cap).expect("容量必须大于 0"))
    }

    /// **定名之测**：读取不保护条目，淘汰只看写入先后。
    ///
    /// 若哪天把 `get` 里的 `peek` 换成 `LruCache::get`，本结构就真成了 LRU，
    /// 届时被淘汰的会是 b 而不是 a，本例失败——那时应当连类型名一起改回去。
    #[test]
    fn evicts_oldest_write_ignoring_reads() {
        let s = store(2);
        s.set("a", Entry::live("a"));
        s.set("b", Entry::live("b"));

        // 疯狂读 a：在 LRU 下这会让 a 成为最近使用、从而免于淘汰
        for _ in 0..10 {
            assert_eq!(s.get("a"), Some(Entry::live("a")));
        }

        // 写入第三个 key 触发淘汰
        s.set("c", Entry::live("c"));

        assert_eq!(s.get("a"), None, "a 写得最早，即便一直被读也应被淘汰");
        assert_eq!(s.get("b"), Some(Entry::live("b")));
        assert_eq!(s.get("c"), Some(Entry::live("c")));
    }

    /// 重复写入会把 key 移到队尾，刷新其淘汰位置。
    #[test]
    fn rewrite_refreshes_eviction_position() {
        let s = store(2);
        s.set("a", Entry::live("a"));
        s.set("b", Entry::live("b"));
        // 重写 a：a 变成「最后写入」，此时最老的是 b
        s.set("a", Entry::live("a2"));
        s.set("c", Entry::live("c"));

        assert_eq!(s.get("b"), None, "重写 a 之后，b 成为最早写入者");
        assert_eq!(s.get("a"), Some(Entry::live("a2")));
        assert_eq!(s.get("c"), Some(Entry::live("c")));
    }

    /// 已过期条目不得被读出（TTL 与容量是两套独立的失效机制）。
    #[test]
    fn expired_entries_are_not_returned() {
        let s = store(2);
        s.set("stale", Entry::dead("stale"));
        s.set("fresh", Entry::live("fresh"));
        assert_eq!(s.get("stale"), None, "过期条目不能被读出");
        assert_eq!(s.get("fresh"), Some(Entry::live("fresh")));
    }

    /// `purge_expired` 必须真正**腾出容量**，而不只是让条目读不到。
    ///
    /// 构造：live 先写（未 purge 时它是最老的、会被优先淘汰），dead 后写。
    /// purge 掉 dead 之后再写一个新 key 就无需淘汰任何东西，live 得以存活；
    /// 若 purge 没有释放槽位，这次写入会把 live 挤掉，本例失败。
    #[test]
    fn purge_expired_frees_capacity_and_keeps_live_entries() {
        let s = store(2);
        s.set("live", Entry::live("live"));
        s.set("dead", Entry::dead("dead"));

        s.purge_expired();
        s.set("fresh", Entry::live("fresh"));

        assert_eq!(
            s.get("live"),
            Some(Entry::live("live")),
            "purge 已腾出 dead 的槽位，新写入不该再淘汰 live"
        );
        assert_eq!(s.get("fresh"), Some(Entry::live("fresh")));
        assert_eq!(s.get("dead"), None);
    }

    /// 读到过期条目时必须顺手腾出槽位，而不只是「读不到」。
    ///
    /// 构造：容量 2，先写 dead（过期）再写 live。读一次 dead 应当把它删掉，
    /// 于是接下来写 fresh 无需淘汰任何东西，live 得以存活。
    /// 若 `get` 只过滤不删除，这次写入会把 live 挤掉。
    #[test]
    fn get_evicts_expired_entry_and_frees_capacity() {
        let s = store(2);
        s.set("dead", Entry::dead("dead"));
        s.set("live", Entry::live("live"));

        assert_eq!(s.get("dead"), None);

        s.set("fresh", Entry::live("fresh"));
        assert_eq!(
            s.get("live"),
            Some(Entry::live("live")),
            "读取过期条目应已腾出槽位，新写入不该再淘汰 live"
        );
        assert_eq!(s.get("fresh"), Some(Entry::live("fresh")));
    }

    #[test]
    fn del_is_idempotent() {
        let s = store(2);
        s.set("k", Entry::live("k"));
        s.del("k");
        assert_eq!(s.get("k"), None);
        // 删不存在的 key 不应 panic
        s.del("k");
        s.del("never-existed");
    }
}
