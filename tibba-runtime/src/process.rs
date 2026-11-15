// Copyright 2026 Tree xie.
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

//! 进程资源采样：CPU / 内存 / 文件描述符 / 磁盘读写。
//!
//! 与 [`crate::AppState`] 同属「进程自身状态」，故与之同 crate；
//! 但 `sysinfo` 传递依赖较重，仅在开启 `process-info` feature 时编译。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessesToUpdate, System};

/// 进程信息缓存容量上限（按 PID 缓存，防高基数 PID 调用无界增长）。
const PROCESS_CACHE_MAX_SIZE: usize = 64;
/// 进程信息缓存有效期（10 秒）。
const PROCESS_CACHE_TTL: Duration = Duration::from_secs(10);

/// 复用的进程信息采样器，跨刷新周期保留前次快照以计算 CPU 使用率及磁盘读写增量。
static PROCESS_SYSTEM: LazyLock<Mutex<System>> = LazyLock::new(|| Mutex::new(System::new()));
/// PID -> `(采样时刻, 快照)` 缓存，替代外部 `cached` 宏以减少传递依赖。
static PROCESS_INFO_CACHE: LazyLock<Mutex<HashMap<usize, (Instant, ProcessSystemInfo)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

/// 获取当前进程的系统资源使用情况，内部委托给 [`get_process_system_info`]。
///
/// ⚠️ **同步阻塞**，async 上下文请用 [`current_process_system_info_async`]。
#[must_use]
pub fn current_process_system_info() -> ProcessSystemInfo {
    get_process_system_info(std::process::id() as usize)
}

/// [`current_process_system_info`] 的 async 版本，采样在阻塞线程池上执行。
pub async fn current_process_system_info_async() -> ProcessSystemInfo {
    get_process_system_info_async(std::process::id() as usize).await
}

/// [`get_process_system_info`] 的 async 版本，采样在阻塞线程池上执行。
///
/// 采样本身是同步系统调用：`System::new()` + `refresh_processes()`，Linux 上
/// `open_files()` 还要遍历 `/proc/<pid>/fd`，量级在毫秒到数十毫秒。更麻烦的是
/// `#[cached]` 的 `sync_writes = "by_key"` 用的是**同步** mutex——缓存冷时并发
/// 调用会一起堵在这把锁上，而它们都跑在 tokio worker 线程上。
///
/// 丢进 `spawn_blocking` 后，阻塞发生在专用线程池，不占用 worker。
/// 阻塞任务 panic 时返回默认值：资源采样是观测数据，不该让请求失败。
pub async fn get_process_system_info_async(pid: usize) -> ProcessSystemInfo {
    tokio::task::spawn_blocking(move || get_process_system_info(pid))
        .await
        .unwrap_or_default()
}

/// 获取指定 PID 进程的系统资源使用情况。
///
/// 结果按 PID 缓存 10 秒（容量上限 64 条）；底层复用持久化的 [`System`] 实例并开启
/// `remove_dead_processes = true`，保证 CPU 百分比与磁盘读写增量有效且已退出进程不累积。
///
/// 采集的指标：内存占用、CPU 使用率与累计时间、文件描述符数量、磁盘读写字节数。
///
/// ⚠️ **同步阻塞**：内部是系统调用。在 async 上下文中请改用 [`get_process_system_info_async`]，
/// 否则会堵住 tokio worker 线程。
pub fn get_process_system_info(pid: usize) -> ProcessSystemInfo {
    let now = Instant::now();
    {
        let cache = PROCESS_INFO_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((cached_at, info)) = cache.get(&pid)
            && now.saturating_duration_since(*cached_at) < PROCESS_CACHE_TTL
        {
            return info.clone();
        }
    }

    let info = {
        let mut sys = PROCESS_SYSTEM
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let sysinfo_pid = Pid::from(pid);
        // remove_dead_processes = true：清理已退出的历史 PID 条目，避免长期运行内存增长
        sys.refresh_processes(ProcessesToUpdate::Some(&[sysinfo_pid]), true);

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
    };

    let mut cache = PROCESS_INFO_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let sampled_at = Instant::now();
    if !cache.contains_key(&pid) && cache.len() >= PROCESS_CACHE_MAX_SIZE {
        cache.retain(|_, (ts, _)| sampled_at.saturating_duration_since(*ts) < PROCESS_CACHE_TTL);
        if cache.len() >= PROCESS_CACHE_MAX_SIZE
            && let Some(oldest_pid) = cache.iter().min_by_key(|(_, (ts, _))| *ts).map(|(k, _)| *k)
        {
            cache.remove(&oldest_pid);
        }
    }
    cache.insert(pid, (sampled_at, info.clone()));
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn current_process_info_reports_nonzero_memory() {
        let info = current_process_system_info_async().await;
        assert!(info.memory_usage > 0);
        // 短时间内第二次读取应直接命中缓存并保持一致的内存读数
        let cached = current_process_system_info_async().await;
        assert_eq!(info.memory_usage, cached.memory_usage);
    }
}
