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

//! 基于 PostgreSQL `FOR UPDATE SKIP LOCKED` 的异步任务队列。
//!
//! 与 cron 调度器（`tibba-scheduler`）互补：cron 管「周期性反复跑」，本模块管
//! 「事件触发、可靠地做掉一次，失败能重试、毒消息隔离」。选 PG 的关键优势是支持
//! 「事务性入队」（[`JobQueue::enqueue_tx`]），杜绝 dual-write 问题。
//!
//! ## 用法
//! 1）实现 [`JobHandler`] 并在启动期 [`register_handler`]：
//! ```ignore
//! struct SendEmail;
//! impl JobHandler for SendEmail {
//!     fn job_type(&self) -> &'static str { "send_email" }
//!     fn handle(&self, ctx: JobContext) -> BoxFuture<'_, Result<()>> {
//!         Box::pin(async move { /* ... */ Ok(()) })
//!     }
//! }
//! register_handler(Arc::new(SendEmail));
//! ```
//! 2）启动 worker：[`start`]`(pool, concurrency)`。
//! 3）入队：[`JobQueue::new`]`(pool).enqueue(&Job::new("send_email", payload)).await`。
//!
//! ## 语义
//! - **至少一次**：handler 必须幂等（同一任务可能因可见性超时被执行两次）。
//! - **重试**：失败按指数退避（+ 由 job id 派生的确定性抖动）推后 `run_at` 重排，
//!   超过 `max_attempts` 转**死信**（`status=3`，保留供排查/重放）。
//! - **崩溃回收**：worker 崩在半路的行（`status=1` 且 `locked_at` 超时）由 reaper 重排。

use dashmap::DashMap;
use metrics::gauge;
use snafu::{ResultExt, Snafu};
use sqlx::types::Json;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tibba_error::Error as BaseError;
use time::PrimitiveDateTime;
use tracing::{error, info, warn};

/// 本 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:job=info`（或 `debug`）过滤。
pub(crate) const LOG_TARGET: &str = "tibba:job";

/// 用于 `dyn JobHandler` 的 Future 返回类型（不引入 async-trait）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

type Result<T> = std::result::Result<T, BaseError>;

// ── 配置常量 ──────────────────────────────────────────────────────────────

/// 队列默认名。
const DEFAULT_QUEUE: &str = "default";
/// 默认重试上限。
const DEFAULT_MAX_ATTEMPTS: i32 = 25;
/// 退避基数（秒）。
const BACKOFF_BASE_SECS: f64 = 5.0;
/// 退避上限（秒）：1 小时封顶。
const BACKOFF_CAP_SECS: f64 = 3600.0;
/// 空队列时的轮询间隔。
const IDLE_POLL: Duration = Duration::from_secs(1);
/// 认领出错（DB 抖动）时的退避。
const CLAIM_ERROR_BACKOFF: Duration = Duration::from_secs(3);
/// reaper 扫描间隔。
const REAP_INTERVAL: Duration = Duration::from_secs(60);
/// 可见性超时：执行中超过此时长未 ack 即视为 worker 崩溃，重排。
const VISIBILITY_TIMEOUT: Duration = Duration::from_secs(300);
/// `last_error` 截断长度，避免异常长报错撑爆列。
const MAX_ERROR_LEN: usize = 2000;
/// 队列深度指标的采样间隔（worker 之外单独一条循环周期性上报）。
const METRICS_INTERVAL: Duration = Duration::from_secs(15);

// 任务状态码（与 jobs.status 列一致）：成功的行直接删除，故无「成功」态。
/// 待跑（可被认领）。
const STATUS_PENDING: i16 = 0;
/// 执行中（已认领未 ack）。
const STATUS_RUNNING: i16 = 1;
/// 死信（超过 max_attempts，保留供排查 / 重放）。
const STATUS_DEAD: i16 = 3;

/// 队列深度 gauge 名；按 `status` label 拆分 pending / running / dead 三条时间线。
/// Prometheus 查询示例：`job_queue_depth{status="dead"}`。
const METRIC_QUEUE_DEPTH: &str = "job_queue_depth";

// ── Handler 注册表 ────────────────────────────────────────────────────────

/// 全局 handler 注册表：job_type → handler。与 `tibba_scheduler::register_job_task`
/// 同思路，撞名时新覆盖旧并打 warn（避免「handler 静默丢失」排查无门）。
static HANDLERS: LazyLock<DashMap<&'static str, Arc<dyn JobHandler>>> = LazyLock::new(DashMap::new);

/// 传给 [`JobHandler::handle`] 的执行上下文。
pub struct JobContext {
    /// 任务记录 id（可作幂等键）。
    pub id: i64,
    /// 当前第几次尝试（认领时已 +1，首次为 1）。
    pub attempt: i32,
    /// 入队时的 payload。
    pub payload: serde_json::Value,
}

/// 任务处理器。`handle` 返回 `Err` 触发重试 / 死信。
///
/// 用于 `dyn` 分发，故返回显式 [`BoxFuture`]（不用 async-trait）。
pub trait JobHandler: Send + Sync {
    /// 处理的任务类型名；入队时按此路由。
    fn job_type(&self) -> &'static str;
    /// 处理一个任务。
    fn handle(&self, ctx: JobContext) -> BoxFuture<'_, Result<()>>;
}

/// 注册一个任务处理器。须在 [`start`] 前调用（通常在启动初始化处）。
pub fn register_handler(handler: Arc<dyn JobHandler>) {
    let job_type = handler.job_type();
    if HANDLERS.insert(job_type, handler).is_some() {
        warn!(
            target: LOG_TARGET,
            job_type,
            "job handler conflict; previous registration overwritten"
        );
    }
}

// ── 任务入队描述 ──────────────────────────────────────────────────────────

/// 入队任务描述。必填项走 [`Job::new`]，可选项走链式 `with_xxx`。
#[derive(Debug, Clone)]
pub struct Job {
    job_type: String,
    payload: serde_json::Value,
    queue: String,
    max_attempts: i32,
    delay: Duration,
}

impl Job {
    /// 新建任务：`job_type` 决定路由的 handler，`payload` 为入参。
    pub fn new(job_type: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            job_type: job_type.into(),
            payload,
            queue: DEFAULT_QUEUE.to_string(),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            delay: Duration::ZERO,
        }
    }

    /// 指定队列名（默认 `default`）。
    #[must_use]
    pub fn with_queue(mut self, queue: impl Into<String>) -> Self {
        self.queue = queue.into();
        self
    }

    /// 指定重试上限（默认 25）。
    #[must_use]
    pub fn with_max_attempts(mut self, max_attempts: i32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    /// 延迟执行：入队后等待 `delay` 才可被认领。
    #[must_use]
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

// ── 认领到的任务 ──────────────────────────────────────────────────────────

/// 从队列认领到的任务行（worker 内部用）。
#[derive(FromRow)]
struct ClaimedJob {
    id: i64,
    job_type: String,
    payload: Json<serde_json::Value>,
    attempts: i32,
    max_attempts: i32,
}

/// 队列深度快照：按状态分别计数（成功的行已删除，不在统计内）。
/// 供 `/metrics` 上报与 admin 概览复用。
#[derive(Debug, Clone, Default)]
pub struct QueueStats {
    /// 待跑条数。
    pub pending: i64,
    /// 执行中条数。
    pub running: i64,
    /// 死信条数。
    pub dead: i64,
}

/// 死信任务行（admin 端点展示 / 重放用）。datetime 保留原始类型，
/// 由调用方决定格式化方式（与项目内 `format_datetime` 约定保持一致）。
#[derive(Debug, FromRow)]
pub struct DeadJob {
    /// 任务 id。
    pub id: i64,
    /// 队列名。
    pub queue: String,
    /// 任务类型。
    pub job_type: String,
    /// 入队 payload。
    pub payload: Json<serde_json::Value>,
    /// 已尝试次数。
    pub attempts: i32,
    /// 重试上限。
    pub max_attempts: i32,
    /// 最近一次失败信息（截断存储）。
    pub last_error: Option<String>,
    /// 转入死信前最后一次计划执行时间。
    pub run_at: PrimitiveDateTime,
    /// 入队时间。
    pub created: PrimitiveDateTime,
}

// ── 队列读写 ──────────────────────────────────────────────────────────────

/// 任务队列：封装对 `jobs` 表的入队 / 认领 / ack / 重试 / 回收。
#[derive(Clone, Copy)]
pub struct JobQueue {
    pool: &'static PgPool,
}

impl JobQueue {
    /// 以给定连接池创建队列句柄。
    pub fn new(pool: &'static PgPool) -> Self {
        Self { pool }
    }

    /// 入队一个任务，返回新任务 id。
    pub async fn enqueue(&self, job: &Job) -> Result<i64> {
        insert_job(self.pool, job).await
    }

    /// 在给定事务内入队（**事务性入队**：与业务写库同事务，杜绝 dual-write）。
    pub async fn enqueue_tx(&self, tx: &mut Transaction<'_, Postgres>, job: &Job) -> Result<i64> {
        insert_job(&mut **tx, job).await
    }

    /// 原子认领一条到点的待跑任务（多 worker 安全，靠 `FOR UPDATE SKIP LOCKED`）。
    async fn claim(&self, worker_id: &str) -> Result<Option<ClaimedJob>> {
        // MVP：跨所有队列取 run_at 最旧的一条；按队列/优先级分流留作后续扩展。
        let row: Option<ClaimedJob> = sqlx::query_as(
            r#"
            WITH next AS (
                SELECT id FROM jobs
                WHERE status = 0 AND run_at <= now()
                ORDER BY run_at
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            UPDATE jobs j
               SET status = 1, locked_at = now(), locked_by = $1, attempts = attempts + 1
              FROM next
             WHERE j.id = next.id
            RETURNING j.id, j.job_type, j.payload, j.attempts, j.max_attempts
            "#,
        )
        .bind(worker_id)
        .fetch_optional(self.pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row)
    }

    /// 成功完成：删除该任务行（成功的行无需保留，避免表膨胀）。
    async fn ack(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM jobs WHERE id = $1")
            .bind(id)
            .execute(self.pool)
            .await
            .context(SqlxSnafu)?;
        Ok(())
    }

    /// 失败处理：未到上限则按退避重排（`status=0`），否则转死信（`status=3`）。
    async fn fail(&self, job: &ClaimedJob, err_msg: &str) -> Result<()> {
        let last_error: String = err_msg.chars().take(MAX_ERROR_LEN).collect();
        if job.attempts < job.max_attempts {
            let backoff = backoff_secs(job.attempts, job.id);
            sqlx::query(
                r#"UPDATE jobs
                      SET status = 0, run_at = now() + make_interval(secs => $2::double precision),
                          locked_at = NULL, locked_by = NULL, last_error = $3
                    WHERE id = $1"#,
            )
            .bind(job.id)
            .bind(backoff)
            .bind(last_error)
            .execute(self.pool)
            .await
            .context(SqlxSnafu)?;
        } else {
            sqlx::query(
                r#"UPDATE jobs
                      SET status = 3, locked_at = NULL, locked_by = NULL, last_error = $2
                    WHERE id = $1"#,
            )
            .bind(job.id)
            .bind(last_error)
            .execute(self.pool)
            .await
            .context(SqlxSnafu)?;
        }
        Ok(())
    }

    /// 回收可见性超时的执行中任务（worker 崩溃兜底），返回重排行数。
    pub async fn reap_stale(&self, visibility: Duration) -> Result<u64> {
        let result = sqlx::query(
            r#"UPDATE jobs
                  SET status = 0, locked_at = NULL, locked_by = NULL
                WHERE status = 1
                  AND locked_at < now() - make_interval(secs => $1::double precision)"#,
        )
        .bind(visibility.as_secs_f64())
        .execute(self.pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.rows_affected())
    }

    /// 队列深度快照：一次 `GROUP BY status` 聚合出各状态条数。
    /// 供指标上报与 admin 概览使用（成功的行已删除，不计入）。
    pub async fn stats(&self) -> Result<QueueStats> {
        let rows: Vec<(i16, i64)> =
            sqlx::query_as("SELECT status, count(*)::bigint FROM jobs GROUP BY status")
                .fetch_all(self.pool)
                .await
                .context(SqlxSnafu)?;
        let mut stats = QueueStats::default();
        for (status, count) in rows {
            match status {
                STATUS_PENDING => stats.pending = count,
                STATUS_RUNNING => stats.running = count,
                STATUS_DEAD => stats.dead = count,
                _ => {}
            }
        }
        Ok(stats)
    }

    /// 分页列出死信任务（按 id 倒序，最近的在前）。`limit` 已由调用方收敛上限。
    pub async fn list_dead(&self, limit: i64, offset: i64) -> Result<Vec<DeadJob>> {
        let rows = sqlx::query_as(
            r#"SELECT id, queue, job_type, payload, attempts, max_attempts, last_error, run_at, created
                 FROM jobs
                WHERE status = 3
                ORDER BY id DESC
                LIMIT $1 OFFSET $2"#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .context(SqlxSnafu)?;
        Ok(rows)
    }

    /// 重放一条死信：`status` 3 → 0、重置 `attempts`、`run_at=now()`、清空错误，
    /// 使其可被立即重新认领。仅作用于死信行；命中返回 `true`。
    pub async fn retry_dead(&self, id: i64) -> Result<bool> {
        let result = sqlx::query(
            r#"UPDATE jobs
                  SET status = 0, attempts = 0, run_at = now(),
                      locked_at = NULL, locked_by = NULL, last_error = NULL
                WHERE id = $1 AND status = 3"#,
        )
        .bind(id)
        .execute(self.pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.rows_affected() > 0)
    }

    /// 永久删除一条死信（确认无需重放后清理）。仅作用于死信行；命中返回 `true`。
    pub async fn purge_dead(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM jobs WHERE id = $1 AND status = 3")
            .bind(id)
            .execute(self.pool)
            .await
            .context(SqlxSnafu)?;
        Ok(result.rows_affected() > 0)
    }
}

/// 入队的实际 INSERT，泛型化 executor 以同时支持连接池与事务。
async fn insert_job<'e, E>(executor: E, job: &Job) -> Result<i64>
where
    E: sqlx::PgExecutor<'e>,
{
    let row: (i64,) = sqlx::query_as(
        r#"INSERT INTO jobs (queue, job_type, payload, max_attempts, run_at)
           VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5::double precision))
           RETURNING id"#,
    )
    .bind(&job.queue)
    .bind(&job.job_type)
    .bind(Json(&job.payload))
    .bind(job.max_attempts)
    .bind(job.delay.as_secs_f64())
    .fetch_one(executor)
    .await
    .context(SqlxSnafu)?;
    Ok(row.0)
}

/// 指数退避秒数：`base * 2^(attempt-1)`，封顶 1 小时，叠加由 job id 派生的确定性
/// 抖动（[0, 20%)），让同批失败任务的重试时间错开，避免 thundering herd。
fn backoff_secs(attempt: i32, id: i64) -> f64 {
    let exp = BACKOFF_BASE_SECS * 2f64.powi((attempt - 1).clamp(0, 12));
    let capped = exp.min(BACKOFF_CAP_SECS);
    let jitter = capped * 0.2 * ((id.unsigned_abs() % 100) as f64 / 100.0);
    capped + jitter
}

// ── Worker / Reaper ───────────────────────────────────────────────────────

/// 启动后台 worker 与 reaper（非阻塞，spawn 后立即返回）。
///
/// `concurrency` 为并发 worker 数（即同时执行的任务上限）。须在 [`register_handler`]
/// 之后调用。注意：MVP 不做优雅排空——进程退出时在跑的任务由 reaper 在
/// 可见性超时后重排（依赖 handler 幂等）。
pub fn start(pool: &'static PgPool, concurrency: usize) {
    let workers = concurrency.max(1);
    for i in 0..workers {
        let worker_id = format!("w-{i}");
        tokio::spawn(worker_loop(pool, worker_id));
    }
    tokio::spawn(reaper_loop(pool));
    tokio::spawn(metrics_loop(pool));
    info!(target: LOG_TARGET, workers, "job workers started");
}

/// 单个 worker 主循环：认领 → 分发 → ack / fail。
async fn worker_loop(pool: &'static PgPool, worker_id: String) {
    let queue = JobQueue::new(pool);
    loop {
        match queue.claim(&worker_id).await {
            Ok(Some(job)) => {
                let outcome = match dispatch(&job).await {
                    Ok(()) => queue.ack(job.id).await,
                    Err(e) => {
                        warn!(
                            target: LOG_TARGET,
                            job_id = job.id,
                            job_type = job.job_type,
                            attempt = job.attempts,
                            error = %e,
                            "job failed"
                        );
                        queue.fail(&job, &e.to_string()).await
                    }
                };
                if let Err(e) = outcome {
                    error!(target: LOG_TARGET, job_id = job.id, error = %e, "persist job state failed");
                }
            }
            // 无活：短睡后再轮询
            Ok(None) => tokio::time::sleep(IDLE_POLL).await,
            // 认领失败（多为 DB 抖动）：退避后重试，避免空转刷错误日志
            Err(e) => {
                error!(target: LOG_TARGET, worker = worker_id, error = %e, "claim job failed");
                tokio::time::sleep(CLAIM_ERROR_BACKOFF).await;
            }
        }
    }
}

/// 按 job_type 路由到注册的 handler 执行；未注册视为失败（走重试 / 死信）。
async fn dispatch(job: &ClaimedJob) -> Result<()> {
    let Some(handler) = HANDLERS.get(job.job_type.as_str()).map(|h| h.value().clone()) else {
        return Err(
            BaseError::new(format!("no handler registered for job type: {}", job.job_type))
                .with_category("job")
                .with_sub_category("no_handler"),
        );
    };
    let ctx = JobContext {
        id: job.id,
        attempt: job.attempts,
        payload: job.payload.0.clone(),
    };
    handler.handle(ctx).await
}

/// reaper 主循环：周期性回收可见性超时的执行中任务。
async fn reaper_loop(pool: &'static PgPool) {
    let queue = JobQueue::new(pool);
    loop {
        tokio::time::sleep(REAP_INTERVAL).await;
        match queue.reap_stale(VISIBILITY_TIMEOUT).await {
            Ok(n) if n > 0 => warn!(target: LOG_TARGET, requeued = n, "reaped stale running jobs"),
            Ok(_) => {}
            Err(e) => error!(target: LOG_TARGET, error = %e, "reap stale jobs failed"),
        }
    }
}

/// 指标主循环：周期性把队列深度按状态上报为 gauge，供 Prometheus 抓取。
///
/// 单独一条循环（而非塞进 reaper）以便用更短的采样间隔；查询很轻（一次
/// 部分索引可覆盖的聚合）。采样失败仅 warn，不影响任务执行。
async fn metrics_loop(pool: &'static PgPool) {
    let queue = JobQueue::new(pool);
    loop {
        tokio::time::sleep(METRICS_INTERVAL).await;
        match queue.stats().await {
            Ok(stats) => {
                gauge!(METRIC_QUEUE_DEPTH, "status" => "pending").set(stats.pending as f64);
                gauge!(METRIC_QUEUE_DEPTH, "status" => "running").set(stats.running as f64);
                gauge!(METRIC_QUEUE_DEPTH, "status" => "dead").set(stats.dead as f64);
            }
            Err(e) => warn!(target: LOG_TARGET, error = %e, "sample queue depth failed"),
        }
    }
}

// ── 错误 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Snafu)]
pub enum Error {
    /// 数据库操作失败。`sqlx::Error` 体积较大，装箱避免 enum 膨胀。
    #[snafu(display("{source}"))]
    Sqlx {
        #[snafu(source(from(sqlx::Error, Box::new)))]
        source: Box<sqlx::Error>,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Sqlx { source } => BaseError::new(source).with_exception(true),
        };
        err.with_category("job")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 退避随尝试次数指数增长且封顶，抖动不超过 20%。
    #[test]
    fn backoff_grows_and_caps() {
        // attempt=1 → base 5s（+抖动）
        let b1 = backoff_secs(1, 0);
        assert!((5.0..6.0).contains(&b1));
        // 充分大的尝试次数被封顶在 [3600, 4320)
        let big = backoff_secs(30, 0);
        assert!((BACKOFF_CAP_SECS..BACKOFF_CAP_SECS * 1.2).contains(&big));
        // 不同 id 抖动不同（错开重试）
        assert!(backoff_secs(5, 1) != backoff_secs(5, 50));
    }

    /// Job 链式配置写入对应字段。
    #[test]
    fn job_builder_sets_fields() {
        let job = Job::new("send_email", serde_json::json!({"to": "x"}))
            .with_queue("email")
            .with_max_attempts(3)
            .with_delay(Duration::from_secs(30));
        assert_eq!(job.job_type, "send_email");
        assert_eq!(job.queue, "email");
        assert_eq!(job.max_attempts, 3);
        assert_eq!(job.delay, Duration::from_secs(30));
    }
}
