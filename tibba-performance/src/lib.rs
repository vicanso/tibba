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

/// 单个进程的系统资源使用快照。
///
/// 所有字节值单位为 bytes，`cpu_usage` 为百分比，范围 `[0, 100]`。
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ProcessSystemInfo {
    /// 内存占用（字节）
    pub memory_usage: u64,
    /// CPU 使用率（百分比，0–100）
    pub cpu_usage: f32,
    /// 累计 CPU 时间（毫秒）
    pub cpu_time: u64,
    /// 打开的文件描述符数量，平台不支持时为 `None`
    pub open_files: Option<usize>,
    /// 进程启动以来写入磁盘的总字节数
    pub total_written_bytes: u64,
    /// 自上次刷新以来写入磁盘的字节数
    pub written_bytes: u64,
    /// 进程启动以来从磁盘读取的总字节数
    pub total_read_bytes: u64,
    /// 自上次刷新以来从磁盘读取的字节数
    pub read_bytes: u64,
}

/// 获取当前进程的系统资源使用情况，内部委托给 `get_process_system_info`。
pub fn current_process_system_info() -> ProcessSystemInfo {
    get_process_system_info(std::process::id() as usize)
}

/// 获取指定 PID 进程的系统资源使用情况。
///
/// 结果按 PID 缓存 10 秒（`sync_writes = "by_key"` 确保同一 PID
/// 同一时刻只有一个线程执行刷新，避免重复采集）。
///
/// 采集的指标：内存占用、CPU 使用率与累计时间、文件描述符数量、磁盘读写字节数。
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
