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
use sysinfo::{Pid, ProcessesToUpdate, System};

/// System resource usage snapshot for a single process.
///
/// All byte values are in bytes; `cpu_usage` is a percentage in `[0, 100]`.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ProcessSystemInfo {
    /// Memory usage in bytes
    pub memory_usage: u64,
    /// CPU usage as a percentage (0–100)
    pub cpu_usage: f32,
    /// Accumulated CPU time in milliseconds
    pub cpu_time: u64,
    /// Number of open file descriptors, if available
    pub open_files: Option<usize>,
    /// Total bytes written to disk since process start
    pub total_written_bytes: u64,
    /// Bytes written to disk since the last refresh
    pub written_bytes: u64,
    /// Total bytes read from disk since process start
    pub total_read_bytes: u64,
    /// Bytes read from disk since the last refresh
    pub read_bytes: u64,
}

/// Returns resource usage for the current process.
///
/// Delegates to [`get_process_system_info`] using the current PID.
pub fn current_process_system_info() -> ProcessSystemInfo {
    get_process_system_info(std::process::id() as usize)
}

/// Returns resource usage for the process identified by `pid`.
///
/// Results are cached for 10 seconds per PID (`sync_writes = "by_key"`
/// ensures only one thread refreshes a given PID at a time).
///
/// Collected metrics:
/// - Memory usage
/// - CPU usage percentage and accumulated CPU time
/// - Open file descriptor count (platform-dependent)
/// - Disk read / write bytes (delta and total)
#[cached(time = 10, sync_writes = "by_key")]
pub fn get_process_system_info(pid: usize) -> ProcessSystemInfo {
    let mut sys = System::new();
    let sysinfo_pid = Pid::from(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[sysinfo_pid]), false);

    sys.process(sysinfo_pid)
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
