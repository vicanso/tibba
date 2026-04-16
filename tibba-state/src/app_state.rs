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

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::time::SystemTime;

/// Thread-safe application state: identity, lifecycle, concurrency, and counters.
pub struct AppState {
    // Service name
    name: String,
    // Semantic version (e.g. "1.2.3")
    version: String,
    // Git commit id
    commit_id: String,
    // Maximum number of concurrent requests allowed; negative means unlimited
    processing_limit: i32,
    // Current application status (running/stopped)
    running: AtomicBool,
    // Current number of requests being processed
    processing: AtomicI32,
    // Historical peak concurrent processing count
    peak_processing: AtomicI32,
    // Total requests handled since startup
    total_requests: AtomicU64,
    // Total error responses (status >= 400) since startup
    error_requests: AtomicU64,
    // Application start timestamp
    started_at: SystemTime,
}

impl AppState {
    /// Creates a new AppState with the given processing limit and commit id.
    pub fn new(processing_limit: i32, commit_id: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            commit_id: commit_id.into(),
            processing_limit,
            running: AtomicBool::new(false),
            processing: AtomicI32::new(0),
            peak_processing: AtomicI32::new(0),
            total_requests: AtomicU64::new(0),
            error_requests: AtomicU64::new(0),
            started_at: SystemTime::now(),
        }
    }

    /// Sets the service name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the semantic version string.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Returns the service name.
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Returns the semantic version string.
    pub fn get_version(&self) -> &str {
        &self.version
    }

    /// Returns the git commit id.
    pub fn get_commit_id(&self) -> &str {
        &self.commit_id
    }

    /// Returns the configured processing limit.
    pub fn get_processing_limit(&self) -> i32 {
        self.processing_limit
    }

    /// Atomically increments the processing counter and updates the peak.
    /// Returns the new value after increment.
    pub fn inc_processing(&self) -> i32 {
        let current = self.processing.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak_processing.fetch_max(current, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        current
    }

    /// Atomically decrements the processing counter.
    /// Returns the new value after decrement.
    pub fn dec_processing(&self) -> i32 {
        self.processing.fetch_sub(1, Ordering::Relaxed) - 1
    }

    /// Returns the current number of requests being processed.
    pub fn get_processing(&self) -> i32 {
        self.processing.load(Ordering::Relaxed)
    }

    /// Returns the historical peak concurrent processing count.
    pub fn get_peak_processing(&self) -> i32 {
        self.peak_processing.load(Ordering::Relaxed)
    }

    /// Returns the total number of requests handled since startup.
    pub fn get_total_requests(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }

    /// Increments the error response counter.
    pub fn inc_error_requests(&self) {
        self.error_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the total number of error responses since startup.
    pub fn get_error_requests(&self) -> u64 {
        self.error_requests.load(Ordering::Relaxed)
    }

    /// Returns true if the application is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Sets the application status to running.
    pub fn run(&self) {
        self.running.store(true, Ordering::Relaxed)
    }

    /// Sets the application status to stopped.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed)
    }

    /// Returns the application start time.
    pub fn get_started_at(&self) -> SystemTime {
        self.started_at
    }
}
