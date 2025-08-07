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
use ctor::ctor;
use once_cell::sync::OnceCell;
use std::time::Duration;
use tibba_cache::{RedisCache, RedisPool, new_redis_pool};
use tibba_error::{Error, new_error};
use tibba_hook::register_before_task;
use tibba_scheduler::{Job, register_job_task};
use tibba_util::new_get_elapsed_ms;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;
static REDIS_CACHE: OnceCell<RedisCache> = OnceCell::new();
static REDIS_POOL: OnceCell<RedisPool> = OnceCell::new();

fn get_redis_pool() -> Result<&'static RedisPool> {
    REDIS_POOL.get_or_try_init(|| {
        let pool = new_redis_pool(&must_get_config().sub_config("redis"))?;
        Ok(pool)
    })
}

pub fn get_redis_cache() -> &'static RedisCache {
    REDIS_CACHE.get_or_init(|| {
        // get redis pool is checked in init function
        // so it can be unwrap here
        let pool = get_redis_pool().unwrap();
        RedisCache::new(pool)
    })
}

async fn redis_health_check() {
    let category = "redis_health_check";
    let elapsed = new_get_elapsed_ms();
    if let Err(e) = get_redis_cache().ping().await {
        error!(category, elapsed = elapsed(), error = %e, "redis unhealthy");
    } else {
        info!(category, elapsed = elapsed(), "redis healthy");
    }
}

#[ctor]
fn init() {
    register_before_task(
        "init_redis_pool",
        16,
        Box::new(|| {
            Box::pin(async {
                let _ = get_redis_pool()?;
                get_redis_cache().ping().await?;
                let job = Job::new_repeated_async(Duration::from_secs(60), |_, _| {
                    Box::pin(redis_health_check())
                })
                .map_err(new_error)?;
                register_job_task("redis_health_check", job);

                let job = Job::new_repeated(Duration::from_secs(60), |_, _| {
                    if let Ok(pool) = get_redis_pool() {
                        let status = pool.status();
                        info!(
                            category = "redis_pool_status",
                            max_size = status.max_size,
                            size = status.size,
                            available = status.available,
                            waiting = status.waiting,
                        );
                    }
                })
                .map_err(new_error)?;
                register_job_task("redis_pool_status", job);
                Ok(())
            })
        }),
    );
}
