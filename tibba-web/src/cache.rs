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
use tibba_cache::{RedisCache, RedisCacheBuilder, RedisPool, new_redis_pool};
use tibba_error::Error;
use tibba_hook::register_before_task;

type Result<T> = std::result::Result<T, Error>;
static REDIS_CACHE: OnceCell<RedisCache> = OnceCell::new();
static REDIS_POOL: OnceCell<RedisPool> = OnceCell::new();

fn get_redis_pool() -> Result<&'static RedisPool> {
    REDIS_POOL.get_or_try_init(|| {
        let redis_config = must_get_config().new_redis_config()?;
        let pool = new_redis_pool(&redis_config)?;
        Ok(pool)
    })
}

pub fn get_redis_cache() -> &'static RedisCache {
    REDIS_CACHE.get_or_init(|| {
        // get redis pool is checked in init function
        // so it can be unwrap here
        let pool = get_redis_pool().unwrap();
        RedisCacheBuilder::new(pool)
            .with_prefix("tibba_web".to_string())
            .build()
    })
}

#[ctor]
fn init() {
    register_before_task(
        "application_cache",
        16,
        Box::new(|| {
            Box::pin(async {
                let _ = get_redis_pool()?;
                Ok(())
            })
        }),
    );
}
