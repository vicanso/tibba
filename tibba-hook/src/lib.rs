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

use async_trait::async_trait;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use tibba_error::Error;
use tracing::info;

// Custom Result type that uses Box<dyn Error> for error handling
type Result<T> = std::result::Result<T, Error>;

#[async_trait]
pub trait Task {
    async fn before(&self) -> Result<bool> {
        Ok(false)
    }
    async fn after(&self) -> Result<bool> {
        Ok(false)
    }
    fn priority(&self) -> u8 {
        0
    }
}

static TASKS: Lazy<DashMap<String, Box<dyn Task + Send + Sync>>> = Lazy::new(DashMap::new);

// Internal function to execute a set of tasks in priority order
// Parameters:
// - tasks: reference to a DashMap containing the tasks to execute
async fn run_tasks(task_type: &str) -> Result<()> {
    // Extract and sort tasks by priority
    let mut names = vec![];
    for item in TASKS.iter() {
        names.push((item.key().clone(), item.value().priority()));
    }

    let is_before = task_type == "before";
    names.sort_by_key(|k| k.1);
    if !is_before {
        names.reverse();
    }

    // Execute tasks in priority order
    for (name, _) in names {
        let Some(item) = TASKS.get(&name) else {
            continue;
        };
        let start = std::time::Instant::now();
        let executed = if is_before {
            item.before().await?
        } else {
            item.after().await?
        };
        if !executed {
            continue;
        }
        info!(
            category = "task",
            task_type,
            name,
            elapsed = start.elapsed().as_millis(),
        );
    }

    Ok(())
}

pub fn register_task(name: &str, task: Box<dyn Task + Send + Sync>) {
    TASKS.insert(name.to_string(), task);
}

// Executes all registered "before" tasks in priority order
pub async fn run_before_tasks() -> Result<()> {
    run_tasks("before").await
}

// Executes all registered "after" tasks in priority order
pub async fn run_after_tasks() -> Result<()> {
    run_tasks("after").await
}
