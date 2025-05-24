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

use dashmap::DashMap;
use once_cell::sync::Lazy;
use tibba_error::{Error, new_error};
use tokio_cron_scheduler::JobScheduler;
use tracing::info;
type Result<T> = std::result::Result<T, Error>;

pub use tokio_cron_scheduler::Job;

pub struct JobTask {
    name: String,
    job: Job,
}

static JOB_TASKS: Lazy<DashMap<String, JobTask>> = Lazy::new(DashMap::new);

pub fn register_job_task(name: &str, job: Job) {
    JOB_TASKS.insert(
        name.to_string(),
        JobTask {
            name: name.to_string(),
            job,
        },
    );
}

pub async fn run_scheduler_jobs() -> Result<JobScheduler> {
    let scheduler = JobScheduler::new()
        .await
        .map_err(|e| new_error(e).with_category("scheduler"))?;
    for job in JOB_TASKS.iter() {
        let value = job.value();
        scheduler
            .add(value.job.clone())
            .await
            .map_err(|e| new_error(e).with_category(&format!("scheduler.{}", value.name)))?;
        info!(category = "scheduler", "add job: {}", value.name);
    }
    scheduler.shutdown_on_ctrl_c();
    scheduler
        .start()
        .await
        .map_err(|err| new_error(err).with_category("scheduler"))?;

    info!(category = "scheduler", "scheduler started");

    Ok(scheduler)
}
