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

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::SystemTime;

/// Application state management structure
/// Provides thread-safe tracking of:
/// - Application status (running/stopped)
/// - Concurrent request processing
/// - Processing limits
/// - Application start time
pub struct AppState {
    // Maximum number of concurrent requests allowed
    processing_limit: i32,
    // Current application status (running/stopped)
    running: AtomicBool,
    // Current number of requests being processed
    processing: AtomicI32,
    // Application start timestamp
    started_at: SystemTime,
    // Application commit id
    commit_id: String,
}

impl AppState {
    /// Creates a new AppState instance with specified processing limit
    pub fn new(processing_limit: i32, commit_id: String) -> Self {
        Self {
            processing_limit,
            running: AtomicBool::new(false),
            processing: AtomicI32::new(0),
            started_at: SystemTime::now(),
            commit_id,
        }
    }

    /// Returns the configured processing limit
    pub fn get_processing_limit(&self) -> i32 {
        self.processing_limit
    }

    /// Returns the application commit id
    pub fn get_commit_id(&self) -> &str {
        &self.commit_id
    }

    /// Atomically increments the processing counter
    /// Returns the previous value
    pub fn inc_processing(&self) -> i32 {
        self.processing.fetch_add(1, Ordering::Relaxed)
    }

    /// Atomically decrements the processing counter
    /// Returns the previous value
    pub fn dec_processing(&self) -> i32 {
        self.processing.fetch_sub(1, Ordering::Relaxed)
    }

    /// Returns the current number of requests being processed
    pub fn get_processing(&self) -> i32 {
        self.processing.load(Ordering::Relaxed)
    }

    /// Checks if the application is currently running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Sets the application status to running
    pub fn run(&self) {
        self.running.store(true, Ordering::Relaxed)
    }

    /// Sets the application status to stopped
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed)
    }

    /// Returns the application start time
    pub fn get_started_at(&self) -> SystemTime {
        self.started_at
    }
}
