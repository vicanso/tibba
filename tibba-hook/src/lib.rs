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
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use tibba_error::Error;
use tracing::info;

/// Tracing target for all log events in this crate.
/// Use `RUST_LOG=tibba-hook=info` (or `debug`) to filter these logs.
const LOG_TARGET: &str = "tibba:hook";

type Result<T> = std::result::Result<T, Error>;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Task {
    fn before(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn after(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn priority(&self) -> u8 {
        0
    }
}

static TASKS: LazyLock<DashMap<String, Arc<dyn Task + Send + Sync>>> = LazyLock::new(DashMap::new);

#[derive(Clone, Copy)]
enum TaskType {
    Before,
    After,
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskType::Before => write!(f, "before"),
            TaskType::After => write!(f, "after"),
        }
    }
}

// Internal function to execute a set of tasks in priority order
// Parameters:
// - tasks: reference to a DashMap containing the tasks to execute
async fn run_tasks(task_type: TaskType) -> Result<()> {
    let mut executable_tasks: Vec<_> = TASKS
        .iter()
        .map(|item| {
            (
                item.key().clone(),      // Task name
                item.value().priority(), // Priority for sorting
                item.value().clone(),    // Cloned Arc to the task
            )
        })
        .collect();

    match task_type {
        TaskType::Before => {
            executable_tasks.sort_by_key(|k| k.1);
        }
        TaskType::After => {
            executable_tasks.sort_by_key(|k| std::cmp::Reverse(k.1));
        }
    }

    // Execute tasks in the sorted order.
    for (name, _, task) in executable_tasks {
        let start = std::time::Instant::now();
        let executed = match task_type {
            TaskType::Before => task.before().await?,
            TaskType::After => task.after().await?,
        };

        if executed {
            info!(
                target: LOG_TARGET,
                task_type = %task_type,
                name,
                elapsed = start.elapsed().as_millis(),
            );
        }
    }

    Ok(())
}

pub fn register_task(name: &str, task: Arc<dyn Task + Send + Sync>) {
    TASKS.insert(name.to_string(), task);
}

// Executes all registered "before" tasks in priority order
pub async fn run_before_tasks() -> Result<()> {
    run_tasks(TaskType::Before).await
}

// Executes all registered "after" tasks in priority order
pub async fn run_after_tasks() -> Result<()> {
    run_tasks(TaskType::After).await
}
