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
        "".to_string()
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

    /// Return the elapsed time in milliseconds since the stopwatch was created.
    pub fn elapsed_ms(&self) -> u32 {
        self.elapsed().as_millis().max(1) as u32
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
