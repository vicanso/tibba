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
use snafu::{ResultExt, Snafu};
use std::sync::LazyLock;
use tibba_error::Error as BaseError;
pub use tokio_cron_scheduler::Job;
use tokio_cron_scheduler::{JobScheduler, JobSchedulerError};
use tracing::info;

type Result<T> = std::result::Result<T, BaseError>;

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("create scheduler failed: {source}"))]
    Create { source: JobSchedulerError },

    #[snafu(display("add job {name} failed: {source}"))]
    AddJob {
        name: String,
        source: JobSchedulerError,
    },

    #[snafu(display("start scheduler failed: {source}"))]
    Start { source: JobSchedulerError },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let message = val.to_string();
        match val {
            Error::AddJob { name, .. } => BaseError::new(message)
                .with_category("scheduler")
                .with_sub_category(name),
            _ => BaseError::new(message).with_category("scheduler"),
        }
    }
}

// name → Job; name is already the key, no need to store it twice in a wrapper struct.
static JOB_TASKS: LazyLock<DashMap<String, Job>> = LazyLock::new(DashMap::new);

/// Register a job task.
///
/// # Arguments
/// * `name` - Unique job name used for logging and error reporting
/// * `job`  - The cron job to register
pub fn register_job_task(name: impl Into<String>, job: Job) {
    JOB_TASKS.insert(name.into(), job);
}

/// Start the scheduler and add all registered jobs.
///
/// # Returns
/// A running [`JobScheduler`] handle.
pub async fn run_scheduler_jobs() -> Result<JobScheduler> {
    let scheduler = JobScheduler::new().await.context(CreateSnafu)?;

    for item in JOB_TASKS.iter() {
        let (name, job) = item.pair();
        scheduler
            .add(job.clone())
            .await
            .context(AddJobSnafu { name: name.clone() })?;
        info!(category = "scheduler", name, "add job success");
    }

    scheduler.shutdown_on_ctrl_c();
    scheduler.start().await.context(StartSnafu)?;

    info!(category = "scheduler", "scheduler started");

    Ok(scheduler)
}
