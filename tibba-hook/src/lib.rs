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
use std::error::Error;
use std::pin::Pin;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

type HookTask = Pin<Box<dyn Future<Output = Result<()>>>>;

// Add Send bound to ensure the future can be safely shared between threads
pub type HookTaskFuture = Box<fn() -> HookTask>;

// Use Send + Sync bound for the stored futures
static HOOK_BEFORE_TASKS: Lazy<DashMap<String, HookTaskFuture>> = Lazy::new(DashMap::new);

// Use Send + Sync bound for the stored futures
static HOOK_AFTER_TASKS: Lazy<DashMap<String, HookTaskFuture>> = Lazy::new(DashMap::new);

// register before task
pub fn register_before_task(name: &str, task: HookTaskFuture) {
    HOOK_BEFORE_TASKS.insert(name.to_string(), task);
}

// register after task
pub fn register_after_task(name: &str, task: HookTaskFuture) {
    HOOK_AFTER_TASKS.insert(name.to_string(), task);
}

// run before tasks
pub async fn run_before_tasks() -> Result<()> {
    for item in HOOK_BEFORE_TASKS.iter() {
        item.value()().await?;
    }
    Ok(())
}

// run after tasks
pub async fn run_after_tasks() -> Result<()> {
    for item in HOOK_AFTER_TASKS.iter() {
        item.value()().await?;
    }
    Ok(())
}
