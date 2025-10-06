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

use cached::proc_macro::cached;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Represents system resource usage information for a process
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ProcessSystemInfo {
    /// Memory usage in bytes
    pub memory_usage: u64,
    /// CPU usage as a percentage (0-100)
    pub cpu_usage: f32,
    /// CPU time in milliseconds
    pub cpu_time: u64,
    /// Open files
    pub open_files: Option<usize>,
    /// Total number of written bytes.
    pub total_written_bytes: u64,
    /// Number of written bytes since the last refresh.
    pub written_bytes: u64,
    /// Total number of read bytes.
    pub total_read_bytes: u64,
    /// Number of read bytes since the last refresh.
    pub read_bytes: u64,
}

/// Retrieves current system resource usage information for this process
///
/// Collects information about:
/// - Memory usage
/// - CPU usage percentage
///
/// # Returns
/// Returns a `ProcessSystemInfo` struct containing the resource usage metrics.
/// If any metrics cannot be retrieved, they will contain default values (0).
#[cached(time = 10, sync_writes = "by_key")]
pub fn get_process_system_info(pid: usize) -> ProcessSystemInfo {
    // Initialize system information collector
    let mut sys = System::new();
    let pid = Pid::from(pid);
    // Refresh CPU usage statistics
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);

    // Get CPU usage for current process if available
    sys.process(pid)
        .map(|process| {
            let disk_usage = process.disk_usage();
            ProcessSystemInfo {
                cpu_usage: process.cpu_usage(),
                memory_usage: process.memory(),
                cpu_time: process.accumulated_cpu_time(),
                open_files: process.open_files(),
                total_written_bytes: disk_usage.total_written_bytes,
                written_bytes: disk_usage.written_bytes,
                total_read_bytes: disk_usage.total_read_bytes,
                read_bytes: disk_usage.read_bytes,
            }
        })
        .unwrap_or_default()
}
