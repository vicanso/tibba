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

use crate::config::must_get_config;
use ctor::ctor;
use once_cell::sync::OnceCell;
use sqlx::MySqlPool;
use std::sync::Arc;
use tibba_error::new_error;
use tibba_hook::register_before_task;
use tibba_sql::{PoolStat, new_mysql_pool};

static DB_POOL: OnceCell<MySqlPool> = OnceCell::new();

pub fn get_db_pool() -> &'static MySqlPool {
    // init db pool is checked in init function
    DB_POOL.get().unwrap()
}

#[ctor]
fn init() {
    register_before_task(
        "init_db_pool",
        16,
        Box::new(|| {
            Box::pin(async {
                let app_config = must_get_config();
                let stat = Arc::new(PoolStat::default());
                let pool = new_mysql_pool(&app_config.sub_config("database"), Some(stat))
                    .await
                    .map_err(|e| new_error(&e.to_string()))?;
                DB_POOL
                    .set(pool)
                    .map_err(|_| new_error("set db pool fail"))?;

                Ok(())
            })
        }),
    );
}
