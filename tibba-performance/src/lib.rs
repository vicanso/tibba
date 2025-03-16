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

use memory_stats::memory_stats;
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, System};

/// Represents system resource usage information for a process
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProcessSystemInfo {
    /// Physical memory usage in bytes
    pub physical_mem: usize,
    /// Virtual memory usage in bytes
    pub virtual_mem: usize,
    /// CPU usage as a percentage (0-100)
    pub cpu_usage: f32,
}

/// Retrieves current system resource usage information for this process
///
/// Collects information about:
/// - Physical memory usage
/// - Virtual memory usage
/// - CPU usage percentage
///
/// # Returns
/// Returns a `ProcessSystemInfo` struct containing the resource usage metrics.
/// If any metrics cannot be retrieved, they will contain default values (0).
pub fn get_process_system_info(pid: usize) -> ProcessSystemInfo {
    // Initialize info struct with default values
    let mut info = ProcessSystemInfo {
        ..Default::default()
    };

    // Get memory statistics if available
    if let Some(usage) = memory_stats() {
        info.physical_mem = usage.physical_mem;
        info.virtual_mem = usage.virtual_mem;
    }

    // Initialize system information collector
    let mut sys = System::new();
    // Refresh CPU usage statistics
    sys.refresh_cpu_usage();

    // Get CPU usage for current process if available
    if let Some(process) = sys.process(Pid::from(pid)) {
        info.cpu_usage = process.cpu_usage();
    }

    info
}
