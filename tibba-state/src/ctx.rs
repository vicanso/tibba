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

use arc_swap::ArcSwap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Trace context for the current request
#[derive(Debug)]
pub struct Context {
    /// Device ID
    pub device_id: String,
    /// Trace ID
    pub trace_id: String,
    /// Start time
    start_time: Instant,
    /// Account
    account: ArcSwap<String>,
}

impl Context {
    pub fn new(device_id: &str, trace_id: &str) -> Self {
        Self {
            device_id: device_id.to_string(),
            trace_id: trace_id.to_string(),
            start_time: Instant::now(),
            account: ArcSwap::new(Arc::new("".to_string())),
        }
    }
    /// Get the elapsed time since the start of the request
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
    /// Get the account
    pub fn get_account(&self) -> Arc<String> {
        self.account.load_full()
    }
    /// Set the account
    pub fn set_account(&self, account: impl Into<String>) {
        self.account.store(Arc::new(account.into()));
    }
}

tokio::task_local! {
    pub static CTX: Arc<Context>;
}
