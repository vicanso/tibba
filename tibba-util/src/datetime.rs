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

pub fn now() -> String {
    Local::now().to_string()
}

pub fn timestamp() -> i64 {
    Local::now().timestamp()
}

pub fn from_timestamp(secs: i64, nsecs: u32) -> String {
    if let Some(value) = DateTime::from_timestamp(secs, nsecs) {
        value.with_timezone(&offset::Local).to_string()
    } else {
        "".to_string()
    }
}

pub fn new_get_duration() -> impl FnOnce() -> Duration {
    let start = Instant::now();
    move || -> Duration { start.elapsed() }
}

pub fn new_get_duration_ms() -> impl FnOnce() -> u32 {
    let start = Instant::now();
    move || -> u32 {
        let value = start.elapsed().as_millis() as u32;
        // the minimum value is 1, avoid the default value
        value.max(1)
    }
}
