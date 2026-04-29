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

/// 当前请求的追踪上下文，记录设备 ID、Trace ID、起始时间和登录账号。
#[derive(Debug)]
pub struct Context {
    /// 设备 ID
    pub device_id: String,
    /// 链路追踪 ID
    pub trace_id: String,
    /// 请求开始时间（用于计算耗时）
    start_time: Instant,
    /// 当前登录账号，无锁原子更新
    account: ArcSwap<String>,
}

impl Context {
    /// 创建新的请求上下文，记录设备 ID 和 Trace ID，并以当前时刻为起始时间。
    pub fn new(device_id: impl Into<String>, trace_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            trace_id: trace_id.into(),
            start_time: Instant::now(),
            account: ArcSwap::new(Arc::new(String::new())),
        }
    }

    /// 返回自请求开始以来经过的时间。
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// 返回自请求开始以来经过的毫秒数。
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    /// 返回当前上下文关联的登录账号。
    pub fn get_account(&self) -> Arc<String> {
        self.account.load_full()
    }

    /// 设置当前上下文的登录账号，无锁原子写入。
    pub fn set_account(&self, account: impl Into<String>) {
        self.account.store(Arc::new(account.into()));
    }
}

tokio::task_local! {
    /// Tokio task-local 变量，存储当前请求的追踪上下文，生命周期与请求任务绑定。
    pub static CTX: Arc<Context>;
}
