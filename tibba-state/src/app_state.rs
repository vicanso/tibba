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

/// 线程安全的应用状态，包含服务标识、生命周期、并发控制和请求计数器。
pub struct AppState {
    /// 服务名称
    name: String,
    /// 语义化版本号（如 "1.2.3"）
    version: String,
    /// Git 提交 ID
    commit_id: String,
    /// 最大并发请求数；负数表示不限制
    processing_limit: i32,
    /// 当前应用运行状态（运行中 / 已停止）
    running: AtomicBool,
    /// 当前正在处理的请求数
    processing: AtomicI32,
    /// 历史最高并发处理数
    peak_processing: AtomicI32,
    /// 启动以来累计处理的请求总数
    total_requests: AtomicU64,
    /// 启动以来累计的错误响应数（状态码 >= 400）
    error_requests: AtomicU64,
    /// 应用启动时间戳
    started_at: SystemTime,
}

impl AppState {
    /// 创建 AppState，传入最大并发限制和 Git 提交 ID，其余字段使用默认值。
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

    /// 设置服务名称，支持链式调用。
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// 设置语义化版本号，支持链式调用。
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// 返回服务名称。
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// 返回语义化版本号。
    pub fn get_version(&self) -> &str {
        &self.version
    }

    /// 返回 Git 提交 ID。
    pub fn get_commit_id(&self) -> &str {
        &self.commit_id
    }

    /// 返回配置的最大并发请求数。
    pub fn get_processing_limit(&self) -> i32 {
        self.processing_limit
    }

    /// 原子性地递增处理计数器，同步更新历史峰值，并累加总请求数。
    /// 返回递增后的当前并发数。
    pub fn inc_processing(&self) -> i32 {
        let current = self.processing.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak_processing.fetch_max(current, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        current
    }

    /// 原子性地递减处理计数器，返回递减后的当前并发数。
    pub fn dec_processing(&self) -> i32 {
        self.processing.fetch_sub(1, Ordering::Relaxed) - 1
    }

    /// 返回当前正在处理的请求数。
    pub fn get_processing(&self) -> i32 {
        self.processing.load(Ordering::Relaxed)
    }

    /// 返回历史最高并发处理数。
    pub fn get_peak_processing(&self) -> i32 {
        self.peak_processing.load(Ordering::Relaxed)
    }

    /// 返回启动以来累计处理的请求总数。
    pub fn get_total_requests(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }

    /// 累加错误响应计数器。
    pub fn inc_error_requests(&self) {
        self.error_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// 返回启动以来累计的错误响应数。
    pub fn get_error_requests(&self) -> u64 {
        self.error_requests.load(Ordering::Relaxed)
    }

    /// 返回 `true` 表示应用当前处于运行状态。
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// 将应用状态设为运行中。
    pub fn run(&self) {
        self.running.store(true, Ordering::Relaxed)
    }

    /// 将应用状态设为已停止。
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed)
    }

    /// 返回应用的启动时间。
    pub fn get_started_at(&self) -> SystemTime {
        self.started_at
    }
}
