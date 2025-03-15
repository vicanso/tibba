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

use chrono::{DateTime, Local, offset};
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
    Local::now().timestamp()
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

/// Creates a closure that measures elapsed time in Duration
///
/// Useful for measuring precise time intervals
///
/// # Returns
/// * Closure that returns elapsed Duration when called
///
/// # Example
/// ```
/// let get_duration = new_get_duration();
/// // ... do some work ...
/// let elapsed = get_duration(); // get elapsed time
/// ```
pub fn new_get_duration() -> impl FnOnce() -> Duration {
    let start = Instant::now();
    move || -> Duration { start.elapsed() }
}

/// Creates a closure that measures elapsed time in milliseconds
///
/// Similar to new_get_duration but returns milliseconds as u32
/// Ensures minimum return value of 1ms to avoid default value confusion
///
/// # Returns
/// * Closure that returns elapsed milliseconds when called
///
/// # Example
/// ```
/// let get_ms = new_get_duration_ms();
/// // ... do some work ...
/// let elapsed_ms = get_ms(); // get elapsed milliseconds
/// ```
pub fn new_get_duration_ms() -> impl FnOnce() -> u32 {
    let start = Instant::now();
    move || -> u32 {
        let value = start.elapsed().as_millis() as u32;
        // Ensure minimum value is 1ms
        value.max(1)
    }
}
