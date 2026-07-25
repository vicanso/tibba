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

//! 进程生命周期编排。
//!
//! 两个模块共享同一形态——全局具名注册表 + 统一驱动执行：
//!
//! | 模块 | 注册 | 驱动 | feature |
//! |------|------|------|---------|
//! | `hook` | [`register_task`] | [`run_before_tasks`] / [`run_after_tasks`] | 始终编译 |
//! | `scheduler` | [`register_job_task`] | `run_scheduler_jobs` | `scheduler` |
//!
//! `hook` 管「启动前 / 关闭后各跑一次」，`scheduler` 管「按 cron 周期性反复跑」。
//! 与 `tibba-job`（Postgres 异步任务队列）的分工：本 crate 是进程内编排，
//! 不落库、不跨进程重试。

mod hook;
#[cfg(feature = "scheduler")]
mod scheduler;

pub use hook::*;
#[cfg(feature = "scheduler")]
pub use scheduler::*;

/// 钩子相关日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:hook=info`（或 `debug`）进行过滤。
pub(crate) const HOOK_LOG_TARGET: &str = "tibba:hook";

/// 调度器相关日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:scheduler=info`（或 `debug`）进行过滤。
///
/// 与 `HOOK_LOG_TARGET` 分开：两者虽同 crate，但排查场景不同
/// （启停问题 vs 定时任务漏跑），合成一个 target 会迫使运维多筛一层。
#[cfg(feature = "scheduler")]
pub(crate) const SCHEDULER_LOG_TARGET: &str = "tibba:scheduler";
