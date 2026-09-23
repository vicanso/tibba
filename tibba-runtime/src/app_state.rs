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

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::SystemTime;

/// 线程安全的应用状态，包含服务标识、生命周期及流控所需的并发计数器。
///
/// 历史上的 `peak_processing` / `total_requests` / `error_requests` 等
/// 统计字段已迁移到 Prometheus（见 `src/metrics.rs` 与 `tibba-middleware`），
/// 这里只保留流控判断必须的 `processing` 原子计数。
pub struct AppState {
    /// 服务名称
    name: String,
    /// 语义化版本号（如 "1.2.3"）
    version: String,
    /// Git 提交 ID
    commit_id: String,
    /// 最大并发请求数；`None` 表示不限制。
    ///
    /// 此前是 `i32` 且「负数 = 不限制」，但应用配置把它校验在 `0..=100000`——
    /// 于是「不限制」根本配不出来，而 `0` 的实际效果是 `1 > 0` → **所有请求 429**。
    /// 用 `Option<NonZeroU32>` 让「不限」与「上限为 N」成为两个类型上不同的状态，
    /// 「上限为 0」这种等于停服的值则无法表达。
    processing_limit: Option<NonZeroU32>,
    /// 当前应用运行状态（运行中 / 已停止）
    running: AtomicBool,
    /// 当前正在处理的请求数；仅在启用了 `processing_limit` 时计数。
    /// 历史峰值、累计请求数等已迁移到 Prometheus，不再在此维护。
    processing: AtomicU32,
    /// 应用启动时间戳
    started_at: SystemTime,
}

impl AppState {
    /// 以 Git 提交 ID 创建 AppState；其余可选项用链式 `with_xxx` 设置。
    ///
    /// 默认不限制并发，见 [`Self::with_processing_limit`]。
    pub fn new(commit_id: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            commit_id: commit_id.into(),
            processing_limit: None,
            running: AtomicBool::new(false),
            processing: AtomicU32::new(0),
            started_at: SystemTime::now(),
        }
    }

    /// 设置最大并发请求数，支持链式调用。`0` 表示不限制。
    ///
    /// 取 `u32` 并把 0 解释为「不限」，与配置文件的直觉一致（`processing_limit = 0`
    /// 关掉流控），而不是把 0 当作一个会拒绝所有请求的上限。
    #[must_use]
    pub fn with_processing_limit(mut self, limit: u32) -> Self {
        self.processing_limit = NonZeroU32::new(limit);
        self
    }

    /// 设置服务名称，支持链式调用。
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// 设置语义化版本号，支持链式调用。
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// 返回服务名称。
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// 返回语义化版本号。
    pub fn get_version(&self) -> &str {
        &self.version
    }

    /// 返回 Git 提交 ID。
    pub fn get_commit_id(&self) -> &str {
        &self.commit_id
    }

    /// 最大并发请求数；`None` 表示不限制。
    pub fn get_processing_limit(&self) -> Option<NonZeroU32> {
        self.processing_limit
    }

    /// 原子性地递增处理计数器，返回递增后的当前并发数。
    /// 仅用于 `processing_limit` 流控判断；统计指标由 Prometheus 负责。
    pub fn inc_processing(&self) -> u32 {
        self.processing
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }

    /// 原子性地递减处理计数器，返回递减后的当前并发数。
    ///
    /// 在 0 上调用时保持为 0，不回绕成 `u32::MAX`——那会让之后每个请求都被
    /// 判为超限。正常路径里 inc / dec 总是成对出现，这只是防御。
    pub fn dec_processing(&self) -> u32 {
        match self
            .processing
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            }) {
            Ok(prev) | Err(prev) => prev.saturating_sub(1),
        }
    }

    /// 返回当前正在处理的请求数。
    pub fn get_processing(&self) -> u32 {
        self.processing.load(Ordering::Relaxed)
    }

    /// 返回 `true` 表示应用当前处于运行状态。
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// 将应用状态设为运行中。
    pub fn run(&self) {
        self.running.store(true, Ordering::Relaxed)
    }

    /// 将应用状态设为已停止。
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed)
    }

    /// 返回应用的启动时间。
    pub fn get_started_at(&self) -> SystemTime {
        self.started_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// 0 表示不限流，而不是「上限为 0」（那等于拒绝所有请求）。
    #[test]
    fn zero_limit_means_unlimited() {
        assert_eq!(AppState::new("c").get_processing_limit(), None);
        assert_eq!(
            AppState::new("c")
                .with_processing_limit(0)
                .get_processing_limit(),
            None
        );
        assert_eq!(
            AppState::new("c")
                .with_processing_limit(8)
                .get_processing_limit()
                .map(NonZeroU32::get),
            Some(8)
        );
    }

    #[test]
    fn counter_inc_dec_round_trip() {
        let state = AppState::new("c");
        assert_eq!(state.inc_processing(), 1);
        assert_eq!(state.inc_processing(), 2);
        assert_eq!(state.dec_processing(), 1);
        assert_eq!(state.dec_processing(), 0);
    }

    /// 不成对的 dec 不得回绕成 u32::MAX——否则之后每个请求都会被判为超限。
    #[test]
    fn unpaired_dec_does_not_wrap() {
        let state = AppState::new("c");
        assert_eq!(state.dec_processing(), 0);
        assert_eq!(state.get_processing(), 0);
        assert_eq!(state.inc_processing(), 1);
    }
}
