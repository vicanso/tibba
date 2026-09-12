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

//! 进程运行时：运行时状态、运行时资源与运行时编排。
//!
//! 三者都描述「正在运行的这个进程」，与请求处理、存储访问无关，故同 crate：
//!
//! | 模块 | 内容 | feature |
//! |------|------|---------|
//! | `app_state` | [`AppState`]：服务标识、运行标志、并发计数、启动时间 | 始终编译 |
//! | `ctx` | task-local 请求上下文 [`CTX`] / [`Context`] | 始终编译 |
//! | `process` | 进程 CPU / 内存 / 文件描述符 / 磁盘读写采样 | `process-info` |
//! | `hook` | 启动前 / 关闭后钩子：[`register_task`] + [`run_before_tasks`] / [`run_after_tasks`] | 始终编译 |
//! | `shutdown` | 进程级停机信号：[`install_shutdown_signal`] + [`shutdown_token`] | 始终编译 |
//! | `scheduler` | cron 定时任务：[`register_job_task`] + `run_scheduler_jobs` | `scheduler` |
//!
//! `shutdown` 管「通知大家该停了」，`hook` 的 `run_after_tasks` 管「大家停完之后
//! 做清理」；`hook` 管「启动/关闭各跑一次」，`scheduler` 管「按 cron 周期性反复跑」。
//! 与 `tibba-job`（Postgres 异步任务队列）的分工：本 crate 是进程内编排，
//! 不落库、不跨进程重试。
//!
//! 两个可选 feature 默认关闭：`sysinfo` 与 `tokio-cron-scheduler` 传递依赖较重，
//! 只用 `AppState` / `CTX` / 钩子的下游不必为其付出编译代价。

mod app_state;
mod ctx;
mod hook;
#[cfg(feature = "process-info")]
mod process;
#[cfg(feature = "scheduler")]
mod scheduler;
mod shutdown;

pub use app_state::*;
pub use ctx::*;
pub use hook::*;
#[cfg(feature = "process-info")]
pub use process::*;
#[cfg(feature = "scheduler")]
pub use scheduler::*;
pub use shutdown::*;

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
