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

//! 应用级共享依赖容器（显式初始化、零 `ctor` 隐式注册）。
//!
//! 启动期调用 [`init`] 按明确顺序初始化：
//! `AppState` -> `PgPool`（含 `./migrations` 自动迁移）-> `RedisClient` / `RedisCache`
//! -> 可选 `Storage`（`file` feature）-> 可选定时监控任务（`scheduler` feature）。

use crate::config::{must_get_basic_config, must_get_config};
use sqlx::PgPool;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tibba_cache::{RedisCache, RedisClient, RedisCmdStat, new_redis_client};
use tibba_error::Error;
use tibba_model::sql::{PoolStat, new_pg_pool};
#[cfg(feature = "file")]
use tibba_opendal::{Storage, new_opendal_storage};
use tibba_router::configuration::ConfigStore;
use tibba_runtime::AppState;
#[cfg(all(feature = "scheduler", feature = "process-info"))]
use tibba_runtime::get_process_system_info_async;
#[cfg(feature = "scheduler")]
use tibba_runtime::{Job, register_job_task, run_scheduler_jobs};
#[cfg(feature = "scheduler")]
use tibba_util::Stopwatch;
#[cfg(feature = "scheduler")]
use tokio::sync::RwLock;
use tracing::{error, info};

type Result<T, E = Error> = std::result::Result<T, E>;

const FALLBACK_SLOW_CMD_THRESHOLD: Duration = Duration::from_millis(200);

static APP_STATE: OnceLock<AppState> = OnceLock::new();
static DB_POOL: OnceLock<PgPool> = OnceLock::new();
static REDIS_CLIENT: OnceLock<RedisClient> = OnceLock::new();
static REDIS_CACHE: OnceLock<RedisCache> = OnceLock::new();
#[cfg(feature = "file")]
static STORAGE: OnceLock<Storage> = OnceLock::new();
static CONFIG_STORE: OnceLock<ConfigStore> = OnceLock::new();
static APP_CTX: OnceLock<AppCtx> = OnceLock::new();

/// 应用级共享依赖集合，由 [`init`] 构造并在进程生命周期内以 `'static` 引用共享。
#[derive(Clone, Copy)]
pub struct AppCtx {
    /// 运行态（并发上限、版本、running 标志）
    pub state: &'static AppState,
    /// PostgreSQL 连接池
    pub pool: &'static PgPool,
    /// Redis 缓存客户端
    pub cache: &'static RedisCache,
    /// 动态配置双层缓存（带跨节点 pub/sub 失效）
    pub configs: &'static ConfigStore,
    /// 对象存储（`file` feature 开启时可用）
    #[cfg(feature = "file")]
    pub storage: &'static Storage,
}

fn redis_cmd_stat(stat: RedisCmdStat) {
    let elapsed = stat.elapsed.as_millis();
    if let Some(error) = stat.error {
        error!(
            target: "tibba:redis",
            cmd = stat.cmd,
            elapsed,
            intentional_block = stat.intentional_block,
            error = error,
            "redis error cmd"
        );
        return;
    }
    if stat.intentional_block {
        return;
    }
    let threshold = REDIS_CLIENT
        .get()
        .map_or(FALLBACK_SLOW_CMD_THRESHOLD, RedisClient::slow_cmd_threshold);
    if stat.elapsed > threshold {
        info!(
            target: "tibba:redis",
            cmd = stat.cmd,
            elapsed,
            threshold = threshold.as_millis(),
            "redis slow cmd"
        );
    }
}

/// 显式初始化全局运行时状态、数据库（自动运行迁移）、Redis、存储与可选监控调度任务。
pub async fn init() -> Result<&'static AppCtx> {
    if let Some(ctx) = APP_CTX.get() {
        return Ok(ctx);
    }

    let app_config = must_get_config();
    let basic_config = must_get_basic_config();

    let state_val = AppState::new(basic_config.commit_id.clone())
        .with_processing_limit(basic_config.processing_limit)
        .with_name("tibba")
        .with_version(env!("CARGO_PKG_VERSION"));
    let _ = APP_STATE.set(state_val);
    let state = APP_STATE
        .get()
        .ok_or_else(|| Error::new("app state init failed").with_category("app_ctx"))?;

    // PostgreSQL + 自动迁移
    let pool_stat = Arc::new(PoolStat::default());
    let pool_val = new_pg_pool(&app_config.sub_config("database"), Some(pool_stat.clone())).await?;
    sqlx::migrate!("./migrations")
        .run(&pool_val)
        .await
        .map_err(|e| {
            Error::new(format!("run migrations fail: {e}"))
                .with_category("sql")
                .with_sub_category("migrate")
                .with_exception(true)
        })?;
    let _ = DB_POOL.set(pool_val);
    let pool = DB_POOL
        .get()
        .ok_or_else(|| Error::new("db pool init failed").with_category("sql"))?;

    // Redis 连接与 ping 预检
    let client_val =
        new_redis_client(&app_config.sub_config("redis"))?.with_stat_callback(&redis_cmd_stat);
    let _ = REDIS_CLIENT.set(client_val);
    let client = REDIS_CLIENT
        .get()
        .ok_or_else(|| Error::new("redis client init failed").with_category("redis"))?;
    let _ = REDIS_CACHE.set(RedisCache::new(client));
    let cache = REDIS_CACHE
        .get()
        .ok_or_else(|| Error::new("redis cache init failed").with_category("redis"))?;
    cache.ping().await?;

    // OpenDAL 对象存储
    #[cfg(feature = "file")]
    let storage = {
        let storage_val = new_opendal_storage(&app_config.sub_config("opendal"))?;
        let info = storage_val.info();
        info!(
            target: "tibba:dal",
            schema = ?info.scheme(),
            capability = ?info.capability(),
            "open dal storage init success"
        );
        let _ = STORAGE.set(storage_val);
        STORAGE
            .get()
            .ok_or_else(|| Error::new("storage init failed").with_category("open_dal"))?
    };

    let _ = CONFIG_STORE.set(ConfigStore::new(pool, client));
    let configs = CONFIG_STORE
        .get()
        .ok_or_else(|| Error::new("config store init failed").with_category("app_ctx"))?;

    // 周期性健康检查与资源指标上报
    #[cfg(feature = "scheduler")]
    {
        register_monitoring_jobs(pool, client, cache, pool_stat)?;
        run_scheduler_jobs().await?;
    }
    #[cfg(not(feature = "scheduler"))]
    let _ = &pool_stat;

    let ctx = AppCtx {
        state,
        pool,
        cache,
        configs,
        #[cfg(feature = "file")]
        storage,
    };
    let _ = APP_CTX.set(ctx);
    APP_CTX
        .get()
        .ok_or_else(|| Error::new("app ctx init failed").with_category("app_ctx"))
}

/// 优雅关闭各底层连接池与运行时调度任务。
pub async fn shutdown(ctx: &AppCtx) {
    ctx.state.stop();
    ctx.configs.stop();
    #[cfg(feature = "scheduler")]
    if let Err(e) = tibba_runtime::run_after_tasks().await {
        tracing::warn!(target: "tibba:app", error = %e, "shutdown scheduler tasks failed");
    }
    if let Some(client) = REDIS_CLIENT.get() {
        client.close();
    }
    ctx.pool.close().await;
}

#[cfg(feature = "scheduler")]
fn register_monitoring_jobs(
    pool: &'static PgPool,
    client: &'static RedisClient,
    cache: &'static RedisCache,
    pool_stat: Arc<PoolStat>,
) -> Result<()> {
    let redis_ping_job = Job::new_repeated_async(Duration::from_secs(60), move |_, _| {
        Box::pin(async move {
            let sw = Stopwatch::new();
            if let Err(e) = cache.ping().await {
                error!(target: "tibba:redis", elapsed = sw.elapsed_ms(), error = %e, "redis unhealthy");
            } else {
                info!(target: "tibba:redis", elapsed = sw.elapsed_ms(), "redis healthy");
            }
        })
    })
    .map_err(Error::new)?;
    register_job_task("redis_health_check", redis_ping_job);

    let redis_stat_job = Job::new_repeated(Duration::from_secs(60), move |_, _| {
        let stat = client.stat();
        info!(
            target: "tibba:redis",
            pool_max_size = stat.pool_max_size,
            pool_size = stat.pool_size,
            pool_available = stat.pool_available,
            pool_waiting = stat.pool_waiting,
            conn_created = stat.conn_created,
            conn_recycled = stat.conn_recycled,
        );
    })
    .map_err(Error::new)?;
    register_job_task("redis_client_stat", redis_stat_job);

    let sql_stat_job = Job::new_repeated(Duration::from_secs(60), move |_, _| {
        let (connected, executions, idle_for) = pool_stat.stat();
        let connection_size = pool.size();
        let connection_idle = pool.num_idle();
        info!(
            target: "tibba:sql",
            category = "database_performance",
            connection_size,
            connection_idle,
            connected,
            executions,
            idle_for,
        );
    })
    .map_err(Error::new)?;
    register_job_task("database_performance", sql_stat_job);

    #[cfg(feature = "process-info")]
    {
        #[derive(Default)]
        struct Performance {
            refresh_count: u32,
            memory_usage_mb: u32,
            cpu_usage: u16,
            cpu_time: u64,
            cpu_time_total: u64,
            open_files: usize,
            written_mb: u32,
            read_mb: u32,
        }
        let perf = Arc::new(RwLock::new(Performance::default()));
        let app_perf_job = Job::new_repeated_async(Duration::from_secs(60), move |_, _| {
            let perf = perf.clone();
            Box::pin(async move {
                let pid = std::process::id() as usize;
                let sys = get_process_system_info_async(pid).await;
                let mb = 1024 * 1024;
                let mut data = perf.write().await;
                data.refresh_count += 1;
                data.memory_usage_mb = (sys.memory_usage / mb) as u32;
                data.cpu_usage = sys.cpu_usage as u16;
                data.cpu_time = sys.cpu_time.saturating_sub(data.cpu_time_total);
                data.cpu_time_total = sys.cpu_time;
                data.open_files = sys.open_files.unwrap_or(0);
                data.written_mb = (sys.written_bytes / mb) as u32;
                data.read_mb = (sys.read_bytes / mb) as u32;
                info!(
                    target: "tibba:state",
                    memory_usage = data.memory_usage_mb,
                    cpu_usage = data.cpu_usage,
                    cpu_time = data.cpu_time,
                    open_files = data.open_files,
                    written_mb = data.written_mb,
                    read_mb = data.read_mb,
                );
            })
        })
        .map_err(Error::new)?;
        register_job_task("application_performance", app_perf_job);
    }

    Ok(())
}
