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

use super::config::must_get_config;
use async_trait::async_trait;
use ctor::ctor;
use once_cell::sync::OnceCell;
use std::sync::Arc;
use std::time::Duration;
use tibba_cache::{RedisCache, RedisClient, RedisCmdStat, new_redis_client};
use tibba_error::Error;
use tibba_hook::{Task, register_task};
use tibba_scheduler::{Job, register_job_task};
use tibba_util::Stopwatch;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;
static REDIS_CACHE: OnceCell<RedisCache> = OnceCell::new();
static REDIS_CLIENT: OnceCell<RedisClient> = OnceCell::new();

fn cmd_stat(stat: RedisCmdStat) {
    let elapsed = stat.elapsed.as_millis();
    let category = "redis_cmd_stat";

    if let Some(error) = stat.error {
        error!(
            category,
            cmd = stat.cmd,
            elapsed,
            error = error,
            "redis error cmd"
        );
    } else if elapsed > 10 {
        info!(category, cmd = stat.cmd, elapsed, "redis slow cmd");
    }
}

fn get_redis_client() -> Result<&'static RedisClient> {
    REDIS_CLIENT.get_or_try_init(|| {
        let mut client = new_redis_client(&must_get_config().sub_config("redis"))?;
        client.with_stat_callback(&cmd_stat);
        Ok(client)
    })
}

pub fn get_redis_cache() -> &'static RedisCache {
    REDIS_CACHE.get_or_init(|| {
        // get redis pool is checked in init function
        // so it can be unwrap here
        let pool = get_redis_client().unwrap();
        RedisCache::new(pool)
    })
}

async fn redis_health_check() {
    let category = "redis_health_check";
    let stopwatch = Stopwatch::new();
    if let Err(e) = get_redis_cache().ping().await {
        error!(category, elapsed = stopwatch.elapsed_ms(), error = %e, "redis unhealthy");
    } else {
        info!(category, elapsed = stopwatch.elapsed_ms(), "redis healthy");
    }
}

struct RedisTask;

#[async_trait]
impl Task for RedisTask {
    async fn before(&self) -> Result<bool> {
        let _ = get_redis_client()?;
        get_redis_cache().ping().await?;
        let job = Job::new_repeated_async(Duration::from_secs(60), |_, _| {
            Box::pin(redis_health_check())
        })
        .map_err(Error::new)?;
        register_job_task("redis_health_check", job);

        let job = Job::new_repeated(Duration::from_secs(60), |_, _| {
            if let Ok(client) = get_redis_client() {
                let status = client.status();
                info!(
                    category = "redis_client_status",
                    max_size = status.max_size,
                    size = status.size,
                    available = status.available,
                    waiting = status.waiting,
                );
            }
        })
        .map_err(Error::new)?;
        register_job_task("redis_client_status", job);
        Ok(true)
    }
    async fn after(&self) -> Result<bool> {
        if let Ok(client) = get_redis_client() {
            client.close();
        }
        Ok(true)
    }
    fn priority(&self) -> u8 {
        16
    }
}

#[ctor]
fn init() {
    register_task("redis", Arc::new(RedisTask));
}
