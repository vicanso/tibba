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
use std::pin::Pin;
use tibba_error::Error;
use tracing::info;

// Custom Result type that uses Box<dyn Error> for error handling
type Result<T> = std::result::Result<T, Error>;

// Type alias for a pinned, boxed Future that returns Result<()>
// Used for async hook tasks
type HookTask = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

// Type alias for hook task functions
// Functions that return a HookTask and can be safely shared between threads
pub type HookTaskFuture = Box<fn() -> HookTask>;

// Global storage for "before" hooks using DashMap for thread-safe concurrent access
// Stores tasks with their priorities as (u8, HookTaskFuture)
// Key: hook name, Value: (priority, task)
static HOOK_BEFORE_TASKS: Lazy<DashMap<String, (u8, HookTaskFuture)>> = Lazy::new(DashMap::new);

// Global storage for "after" hooks using DashMap for thread-safe concurrent access
// Similar structure to HOOK_BEFORE_TASKS
static HOOK_AFTER_TASKS: Lazy<DashMap<String, (u8, HookTaskFuture)>> = Lazy::new(DashMap::new);

// Registers a task to be executed before the main operation
// Parameters:
// - name: unique identifier for the task
// - priority: execution order (lower numbers execute first)
// - task: the function to be executed
pub fn register_before_task(name: &str, priority: u8, task: HookTaskFuture) {
    HOOK_BEFORE_TASKS.insert(name.to_string(), (priority, task));
}

// Registers a task to be executed after the main operation
// Parameters:
// - name: unique identifier for the task
// - priority: execution order (lower numbers execute first)
// - task: the function to be executed
pub fn register_after_task(name: &str, priority: u8, task: HookTaskFuture) {
    HOOK_AFTER_TASKS.insert(name.to_string(), (priority, task));
}

// Internal function to execute a set of tasks in priority order
// Parameters:
// - tasks: reference to a DashMap containing the tasks to execute
async fn run_task(tasks: &DashMap<String, (u8, HookTaskFuture)>) -> Result<()> {
    // Extract and sort tasks by priority
    let mut names = vec![];
    for item in tasks.iter() {
        names.push((item.key().clone(), item.value().0));
    }
    names.sort_by_key(|k| k.1);

    let category = "run_task";
    // Execute tasks in priority order
    for (name, _) in names {
        info!(category, name, "start to run task");
        if let Some(item) = tasks.get(&name) {
            item.1().await?;
        }
        info!(category, name, "run task success");
    }

    Ok(())
}

// Executes all registered "before" tasks in priority order
pub async fn run_before_tasks() -> Result<()> {
    run_task(&HOOK_BEFORE_TASKS).await
}

// Executes all registered "after" tasks in priority order
pub async fn run_after_tasks() -> Result<()> {
    run_task(&HOOK_AFTER_TASKS).await
}

// Example usage of the hook system
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    async fn example_before_task() -> Result<()> {
        // Simulate some async work
        sleep(Duration::from_millis(100)).await;
        info!("Example before task executed");
        Ok(())
    }

    async fn example_after_task() -> Result<()> {
        // Simulate some async work
        sleep(Duration::from_millis(50)).await;
        info!("Example after task executed");
        Ok(())
    }

    #[tokio::test]
    async fn test_hook_system() -> Result<()> {
        // Register before tasks with different priorities
        register_before_task(
            "validation",
            1,
            Box::new(|| Box::pin(example_before_task())),
        );

        register_before_task("logging", 2, Box::new(|| Box::pin(example_before_task())));

        // Register after tasks
        register_after_task("cleanup", 1, Box::new(|| Box::pin(example_after_task())));

        // Execute hooks
        run_before_tasks().await?;
        // ... main operation would happen here ...
        run_after_tasks().await?;

        Ok(())
    }
}
