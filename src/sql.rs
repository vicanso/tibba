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
use async_trait::async_trait;
use ctor::ctor;
use once_cell::sync::OnceCell;
use sqlx::MySqlPool;
use std::sync::atomic::Ordering;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;
use tibba_error::Error;
use tibba_hook::{Task, register_task};
use tibba_scheduler::{Job, register_job_task};
use tibba_sql::{PoolStat, new_mysql_pool};
use tracing::info;

type Result<T> = std::result::Result<T, Error>;
static DB_POOL: OnceCell<MySqlPool> = OnceCell::new();

pub fn get_db_pool() -> &'static MySqlPool {
    // init db pool is checked in init function
    DB_POOL
        .get()
        .unwrap_or_else(|| panic!("db pool not initialized"))
}

struct SqlTask {
    running: AtomicBool,
}

#[async_trait]
impl Task for SqlTask {
    async fn before(&self) -> Result<bool> {
        let app_config = must_get_config();
        let stat = Arc::new(PoolStat::default());
        let pool = new_mysql_pool(&app_config.sub_config("database"), Some(stat.clone()))
            .await
            .map_err(Error::new)?;
        DB_POOL
            .set(pool)
            .map_err(|_| Error::new("set db pool fail"))?;

        let task = "database_performance";
        let job = Job::new_repeated(Duration::from_secs(60), move |_, _| {
            let (connected, executions, idle_for) = stat.stat();
            let pool = get_db_pool();
            let connection_size = pool.size();
            let connection_idle = pool.num_idle();

            info!(
                category = task,
                connection_size, connection_idle, connected, executions, idle_for,
            );
        })
        .map_err(Error::new)?;
        register_job_task(task, job);
        self.running.store(true, Ordering::Relaxed);

        Ok(true)
    }
    async fn after(&self) -> Result<bool> {
        if !self.running.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let pool = get_db_pool();
        pool.close().await;
        Ok(true)
    }
    fn priority(&self) -> u8 {
        16
    }
}

#[ctor]
fn init() {
    register_task(
        "sql",
        Arc::new(SqlTask {
            running: AtomicBool::new(false),
        }),
    );
}
