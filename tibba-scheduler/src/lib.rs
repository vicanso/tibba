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

use dashmap::DashMap;
use snafu::{ResultExt, Snafu};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tibba_error::Error as BaseError;
pub use tokio_cron_scheduler::Job;
use tokio_cron_scheduler::{JobScheduler, JobSchedulerError};
use tracing::{error, info, warn};

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:scheduler=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:scheduler";

type Result<T> = std::result::Result<T, BaseError>;

#[derive(Debug, Snafu)]
enum Error {
    /// 创建调度器实例失败。
    #[snafu(display("create scheduler failed: {source}"))]
    Create { source: JobSchedulerError },

    /// 向调度器添加指定名称的任务失败。
    #[snafu(display("add job {name} failed: {source}"))]
    AddJob {
        name: String,
        source: JobSchedulerError,
    },

    /// 启动调度器失败。
    #[snafu(display("start scheduler failed: {source}"))]
    Start { source: JobSchedulerError },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Create { source } => BaseError::new(source),
            Error::AddJob { name, source } => BaseError::new(source).with_sub_category(name),
            Error::Start { source } => BaseError::new(source),
        };
        err.with_category("scheduler")
    }
}

/// 分布式锁获取回调返回的 Future：`true` = 本实例抢到锁（应执行任务），
/// `false` = 未抢到（集群中已有实例在跑，跳过本次触发）。
pub type LockFuture = Pin<Box<dyn Future<Output = bool> + Send + 'static>>;

/// 分布式锁获取回调：入参为锁键与 TTL，返回 [`LockFuture`]。
///
/// 由调用方注入（通常以 `RedisCache::lock` 实现），让本 crate 不直接依赖 Redis——
/// 与 `tibba-router-common` 中 `ReadinessCheck` 的注入方式一致。
pub type TryLock = Arc<dyn Fn(String, Duration) -> LockFuture + Send + Sync>;

/// 构造一个「全集群单实例」的 cron 异步任务。
///
/// 每次触发先用注入的分布式锁 `lock` 抢占 `lock_key`，仅抢到锁的实例执行 `body`，
/// 其余实例跳过本次触发——解决多副本部署时同一定时任务被每个副本重复执行的问题。
///
/// `lock_ttl` 的取值需同时满足：
/// - **小于** cron 触发间隔：使锁在下次触发前过期，各实例可重新公平竞争；
/// - **大于** `body` 的预期执行时长：避免执行中途锁过期被另一实例并发接管。
///
/// 与异步任务队列一致，`body` 仍应保持幂等：锁基于 TTL，执行时长超过 TTL 的极端
/// 情况下仍可能并发。`lock` 获取失败（如 Redis 不可用）时由注入方决定返回值，
/// 返回 `false` 即本次全体跳过——「宁可少跑一次，也不要多实例重复执行」。
pub fn singleton_cron_job<F, Fut>(
    schedule: &str,
    lock: TryLock,
    lock_key: impl Into<String>,
    lock_ttl: Duration,
    body: F,
) -> std::result::Result<Job, JobSchedulerError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let lock_key = lock_key.into();
    // body 可能被多次触发调用，用 Arc 在每次闭包调用时共享而非移动消耗
    let body = Arc::new(body);
    Job::new_async(schedule, move |_uuid, _scheduler| {
        let lock = lock.clone();
        let lock_key = lock_key.clone();
        let body = body.clone();
        Box::pin(async move {
            // 抢到分布式锁的实例才执行；未抢到说明集群中已有实例在跑，跳过本次触发
            if lock(lock_key, lock_ttl).await {
                body().await;
            }
        })
    })
}

/// 全局任务注册表，键为任务名称，值为 cron Job 实例。
/// 名称已作为 key 存储，无需在值中重复保存。
static JOB_TASKS: LazyLock<DashMap<String, Job>> = LazyLock::new(DashMap::new);

/// 注册一个定时任务。
/// `name` 须唯一，用于日志输出和错误定位；同名注册时新任务覆盖旧任务，
/// 并打 warn 日志——两个 ctor 块意外撞名是常见的「调度任务静默丢失」根因，
/// 这里显式提示，避免操作员排查无门。
pub fn register_job_task(name: impl Into<String>, job: Job) {
    let name = name.into();
    if JOB_TASKS.insert(name.clone(), job).is_some() {
        warn!(
            target: LOG_TARGET,
            name,
            "job task name conflict; previous registration overwritten"
        );
    }
}

/// 启动调度器，将所有已注册的定时任务逐一添加并开始运行。
/// 注册 Ctrl-C 信号处理，进程退出时自动优雅关闭调度器。
/// 返回正在运行的 `JobScheduler` 句柄，调用方可持有以便后续管理。
/// 任一任务添加失败均 fail-fast——启动期半数任务在跑是更糟糕的状态。
pub async fn run_scheduler_jobs() -> Result<JobScheduler> {
    let scheduler = JobScheduler::new().await.context(CreateSnafu)?;

    let mut added = 0_usize;
    for item in JOB_TASKS.iter() {
        let (name, job) = item.pair();
        if let Err(err) = scheduler.add(job.clone()).await {
            // 失败任务在 fail-fast 前先记一条带 name 的错误日志；
            // 让操作员一眼看到是哪个任务在启动期掉链子，无需再去 grep snafu Display
            error!(
                target: LOG_TARGET,
                name,
                error = %err,
                "add job failed",
            );
            return Err(BaseError::from(Error::AddJob {
                name: name.clone(),
                source: err,
            }));
        }
        info!(target: LOG_TARGET, name, "add job success");
        added += 1;
    }

    scheduler.shutdown_on_ctrl_c();
    scheduler.start().await.context(StartSnafu)?;

    info!(target: LOG_TARGET, jobs = added, "scheduler started");

    Ok(scheduler)
}
