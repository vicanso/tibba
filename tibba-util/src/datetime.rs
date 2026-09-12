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

use chrono::{DateTime, Local, Utc, offset};
use std::time::{Duration, Instant};

/// Returns the current local time as a formatted string
///
/// Uses the system's local timezone for formatting
/// Format example: "2025-01-01 12:00:00.000 +08:00"
pub fn now() -> String {
    Local::now().to_string()
}

/// Returns the current Unix timestamp in seconds
///
/// Represents seconds elapsed since Unix epoch (1970-01-01 00:00:00 UTC)
pub fn timestamp() -> i64 {
    Utc::now().timestamp()
}

/// Converts a Unix timestamp to a formatted local datetime string
///
/// # Arguments
/// * `secs` - Seconds since Unix epoch
/// * `nsecs` - Nanoseconds component
///
/// # Returns
/// * Formatted datetime string in local timezone
/// * Empty string if timestamp is invalid
pub fn from_timestamp(secs: i64, nsecs: u32) -> String {
    if let Some(value) = DateTime::from_timestamp(secs, nsecs) {
        value.with_timezone(&offset::Local).to_string()
    } else {
        String::new()
    }
}

/// A stopwatch structure for measuring elapsed time.
#[derive(Debug, Clone, Copy)]
pub struct Stopwatch {
    start: Instant,
}

impl Stopwatch {
    /// Create a new stopwatch instance and start timing.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Return the elapsed time since the stopwatch was created.
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

    /// Return a human-readable elapsed time string.
    pub fn elapsed_human(&self) -> String {
        humantime::format_duration(self.elapsed()).to_string()
    }
}

/// Implement the Default trait for Stopwatch, allowing it to be created via `Stopwatch::default()`.
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
}
