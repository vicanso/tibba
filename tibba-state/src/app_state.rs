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

use std::sync::atomic::{AtomicI8, AtomicI32, Ordering};
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
    status: AtomicI8,
    // Current number of requests being processed
    processing: AtomicI32,
    // Application start timestamp
    started_at: SystemTime,
}

// Application status constants
const APP_STATUS_STOP: i8 = 0; // Application is stopped
const APP_STATUS_RUNNING: i8 = 1; // Application is running

impl AppState {
    /// Creates a new AppState instance with specified processing limit
    pub fn new(processing_limit: i32) -> Self {
        Self {
            processing_limit,
            status: AtomicI8::new(APP_STATUS_STOP),
            processing: AtomicI32::new(0),
            started_at: SystemTime::now(),
        }
    }

    /// Returns the configured processing limit
    pub fn get_processing_limit(&self) -> i32 {
        self.processing_limit
    }

    /// Atomically increments the processing counter
    /// Returns the previous value
    pub fn inc_processing(&self) -> i32 {
        self.processing.fetch_add(1, Ordering::Relaxed)
    }

    /// Atomically decrements the processing counter
    /// Returns the previous value
    pub fn dec_processing(&self) -> i32 {
        self.processing.fetch_add(-1, Ordering::Relaxed)
    }

    /// Returns the current number of requests being processed
    pub fn get_processing(&self) -> i32 {
        self.processing.load(Ordering::Relaxed)
    }

    /// Checks if the application is currently running
    pub fn is_running(&self) -> bool {
        let value = self.status.load(Ordering::Relaxed);
        value == APP_STATUS_RUNNING
    }

    /// Sets the application status to running
    pub fn run(&self) {
        self.status.store(APP_STATUS_RUNNING, Ordering::Relaxed)
    }

    /// Sets the application status to stopped
    pub fn stop(&self) {
        self.status.store(APP_STATUS_STOP, Ordering::Relaxed)
    }

    /// Returns the application start time
    pub fn get_started_at(&self) -> SystemTime {
        self.started_at
    }
}
