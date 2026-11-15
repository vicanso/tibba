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

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// 返回当前 UTC 时间的 RFC3339 字符串，例如 `"2025-01-01T04:00:00Z"`。
pub fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// 返回当前 Unix 时间戳（自 UTC 1970-01-01 00:00:00 起的秒数）。
pub fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// 将 Unix 时间戳转换为 UTC RFC3339 日期时间字符串；时间戳或纳秒非法时返回空串。
///
/// # 参数
/// * `secs` - Unix 秒数
/// * `nsecs` - 纳秒偏移（合法范围 `0..1_000_000_000`）
pub fn from_timestamp(secs: i64, nsecs: u32) -> String {
    if nsecs >= 1_000_000_000 {
        return String::new();
    }
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs) else {
        return String::new();
    };
    let Some(dt) = dt.checked_add(time::Duration::nanoseconds(i64::from(nsecs))) else {
        return String::new();
    };
    dt.format(&Rfc3339).unwrap_or_default()
}

/// 用于统计耗时的单调秒表。
#[derive(Debug, Clone, Copy)]
pub struct Stopwatch {
    start: Instant,
}

impl Stopwatch {
    /// 创建并立即启动秒表。
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// 返回自秒表创建以来经过的 [`Duration`]。
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// 自创建以来经过的毫秒数。
    ///
    /// 不足 1ms 返回 `0`，这是真实值。此前这里有 `.max(1)`，把所有亚毫秒操作
    /// 统一抬成 1ms——缓存命中、本地反序列化这类耗时全部失真（`HttpStats.serde`
    /// 几乎永远是 1），而统计的意义正在于分辨快慢。需要亚毫秒精度请用
    /// [`Self::elapsed_us`]。
    ///
    /// 超过 `u32::MAX` 毫秒（约 49 天）时饱和，不回绕。
    pub fn elapsed_ms(&self) -> u32 {
        u32::try_from(self.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    /// 自创建以来经过的微秒数，用于毫秒精度不够的场景。
    ///
    /// 超过 `u64::MAX` 微秒（约 58 万年）时饱和。
    pub fn elapsed_us(&self) -> u64 {
        u64::try_from(self.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    /// 返回人类可读的耗时字符串（如 `"12ms"`）。
    pub fn elapsed_human(&self) -> String {
        humantime::format_duration(self.elapsed()).to_string()
    }
}

/// 为 `Stopwatch` 提供默认实现，等价于 [`Stopwatch::new`]。
impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// 回归守卫：亚毫秒必须如实报 0，不得被抬成 1。
    #[test]
    fn sub_millisecond_reports_zero_not_one() {
        let sw = Stopwatch::new();
        // 刚创建，耗时远小于 1ms
        assert_eq!(
            sw.elapsed_ms(),
            0,
            "亚毫秒操作应报 0，此前被 .max(1) 抬成 1"
        );
    }

    #[test]
    fn microsecond_resolution_is_available() {
        let sw = Stopwatch::new();
        std::thread::sleep(Duration::from_millis(2));
        assert!(sw.elapsed_ms() >= 2);
        assert!(
            sw.elapsed_us() >= 2_000,
            "微秒读数应至少与毫秒读数一致，实际 {}",
            sw.elapsed_us()
        );
    }

    #[test]
    fn elapsed_is_monotonic() {
        let sw = Stopwatch::new();
        let first = sw.elapsed_us();
        std::thread::sleep(Duration::from_millis(1));
        assert!(sw.elapsed_us() > first);
    }

    #[test]
    fn datetime_helpers_format_and_reject_invalid_nsecs() {
        assert_eq!(from_timestamp(0, 0), "1970-01-01T00:00:00Z");
        assert_eq!(from_timestamp(0, 1_000_000_000), "");
        assert!(!now().is_empty());
        assert!(timestamp() > 0);
    }
}
