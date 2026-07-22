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

use super::config::must_get_config;
use ctor::ctor;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tibba_cache::{RedisCache, RedisClient, RedisCmdStat, new_redis_client};
use tibba_error::Error;
use tibba_hook::{BoxFuture, Task, register_task};
use tibba_scheduler::{Job, LockFuture, TryLock, register_job_task};
use tibba_util::Stopwatch;
use tracing::{error, info, warn};

/// 本模块所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:redis=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:redis";

type Result<T> = std::result::Result<T, Error>;
static REDIS_CACHE: OnceLock<RedisCache> = OnceLock::new();
static REDIS_CLIENT: OnceLock<RedisClient> = OnceLock::new();

/// 慢命令阈值兜底：客户端尚未就绪时使用，正常路径取 URI `slow=`。
const FALLBACK_SLOW_CMD_THRESHOLD: Duration = Duration::from_millis(200);

fn cmd_stat(stat: RedisCmdStat) {
    let elapsed = stat.elapsed.as_millis();

    if let Some(error) = stat.error {
        error!(
            target: LOG_TARGET,
            cmd = stat.cmd,
            elapsed,
            intentional_block = stat.intentional_block,
            error = error,
            "redis error cmd"
        );
        return;
    }
    // 意图性阻塞命令（BRPOP / XREAD BLOCK 等）本就该久等，不计入慢命令，
    // 否则它会长期霸榜把真正的慢查询淹掉
    if stat.intentional_block {
        return;
    }
    // 阈值取 URI `slow=`（默认 200ms），不再硬编码
    let threshold = REDIS_CLIENT
        .get()
        .map_or(FALLBACK_SLOW_CMD_THRESHOLD, RedisClient::slow_cmd_threshold);
    if stat.elapsed > threshold {
        info!(
            target: LOG_TARGET,
            cmd = stat.cmd,
            elapsed,
            threshold = threshold.as_millis(),
            "redis slow cmd"
        );
    }
}

fn get_redis_client() -> Result<&'static RedisClient> {
    // OnceLock::get_or_try_init 尚未稳定；先 get，未初始化再构造 + set（竞态时取胜者）
    if let Some(client) = REDIS_CLIENT.get() {
        return Ok(client);
    }
    let client =
        new_redis_client(&must_get_config().sub_config("redis"))?.with_stat_callback(&cmd_stat);
    let _ = REDIS_CLIENT.set(client);
    REDIS_CLIENT
        .get()
        .ok_or_else(|| Error::new("redis client not initialized").with_category("redis"))
}

pub fn get_redis_cache() -> &'static RedisCache {
    REDIS_CACHE.get_or_init(|| {
        // get redis pool is checked in init function
        // so it can be unwrap here
        let pool =
            get_redis_client().unwrap_or_else(|e| panic!("redis client not initialized: {e:?}"));
        RedisCache::new(pool)
    })
}

/// 返回基于全局 Redis 的分布式锁获取回调，供 scheduler 的 `singleton_cron_job` 使用。
///
/// 多副本部署时，仅抢到锁（SET NX）的实例执行该次定时任务，其余实例跳过本次触发。
/// 抢锁失败（通常意味着 Redis 不可用）时返回 `false`：本次全体跳过，
/// 「宁可少跑一次，也不要多副本重复执行」，并打 warn 便于排障。
///
/// 当前仅 `demo-detector` / `demo-docker` 样板任务使用；关 feature 时保留 API 供业务接入。
#[cfg_attr(
    not(any(feature = "demo-detector", feature = "demo-docker")),
    allow(dead_code)
)]
pub fn redis_try_lock() -> TryLock {
    Arc::new(|key: String, ttl: Duration| -> LockFuture {
        Box::pin(async move {
            match get_redis_cache().lock(&key, Some(ttl)).await {
                Ok(acquired) => acquired,
                Err(e) => {
                    warn!(
                        target: LOG_TARGET,
                        key,
                        error = %e,
                        "acquire singleton job lock failed; skipping this tick"
                    );
                    false
                }
            }
        })
    })
}

async fn redis_health_check() {
    let stopwatch = Stopwatch::new();
    if let Err(e) = get_redis_cache().ping().await {
        error!(target: LOG_TARGET, elapsed = stopwatch.elapsed_ms(), error = %e, "redis unhealthy");
    } else {
        info!(target: LOG_TARGET, elapsed = stopwatch.elapsed_ms(), "redis healthy");
    }
}

struct RedisTask;

impl Task for RedisTask {
    fn before(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            let _ = get_redis_client()?;
            get_redis_cache().ping().await?;
            let job = Job::new_repeated_async(Duration::from_secs(60), |_, _| {
                Box::pin(redis_health_check())
            })
            .map_err(Error::new)?;
            register_job_task("redis_health_check", job);

            let job = Job::new_repeated(Duration::from_secs(60), |_, _| {
                if let Ok(client) = get_redis_client() {
                    let stat = client.stat();
                    info!(
                        target: LOG_TARGET,
                        pool_max_size = stat.pool_max_size,
                        pool_size = stat.pool_size,
                        pool_available = stat.pool_available,
                        pool_waiting = stat.pool_waiting,
                        conn_created = stat.conn_created,
                        conn_recycled = stat.conn_recycled,
                    );
                }
            })
            .map_err(Error::new)?;
            register_job_task("redis_client_stat", job);
            Ok(true)
        })
    }
    fn after(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            if let Ok(client) = get_redis_client() {
                client.close();
            }
            Ok(true)
        })
    }
    fn priority(&self) -> u8 {
        16
    }
}

#[ctor(unsafe)]
fn init() {
    register_task("redis", Arc::new(RedisTask));
}
