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

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:hook=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:hook";

type Result<T> = std::result::Result<T, Error>;

/// 装箱的异步 Future，用于 trait object 场景下的异步方法返回类型。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 生命周期钩子 trait，用于在应用启动/关闭时执行自定义逻辑。
///
/// - `before`：应用启动前执行（如初始化资源），按优先级从低到高顺序调用。
/// - `after`：应用关闭后执行（如释放资源），按优先级从高到低顺序调用。
/// - 返回 `true` 表示该钩子实际执行了操作，会记录耗时日志；返回 `false` 则静默跳过。
pub trait Task {
    /// 应用启动前的钩子，默认不执行任何操作。
    fn before(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    /// 应用关闭后的钩子，默认不执行任何操作。
    fn after(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    /// 执行优先级，数值越小优先级越高（before 阶段），after 阶段反之。默认为 0。
    fn priority(&self) -> u8 {
        0
    }
}

/// 全局任务注册表，键为任务名称，值为线程安全的任务实例。
static TASKS: LazyLock<DashMap<String, Arc<dyn Task + Send + Sync>>> = LazyLock::new(DashMap::new);

/// 任务执行阶段：启动前（Before）或关闭后（After）。
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

/// 按优先级顺序执行所有已注册的钩子任务。
/// Before 阶段按优先级升序（数值小的先执行），After 阶段按降序。
/// 任务返回 `true` 时记录耗时日志。
async fn run_tasks(task_type: TaskType) -> Result<()> {
    let mut executable_tasks: Vec<_> = TASKS
        .iter()
        .map(|item| {
            (
                item.key().clone(),      // 任务名称
                item.value().priority(), // 排序优先级
                item.value().clone(),    // 任务实例的 Arc 克隆
            )
        })
        .collect();

    match task_type {
        TaskType::Before => {
            // 启动前：优先级数值小的先执行
            executable_tasks.sort_by_key(|k| k.1);
        }
        TaskType::After => {
            // 关闭后：优先级数值大的先执行（与 before 相反）
            executable_tasks.sort_by_key(|k| std::cmp::Reverse(k.1));
        }
    }

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

/// 注册一个具名钩子任务。同名任务重复注册时，新任务会覆盖旧任务。
pub fn register_task(name: &str, task: Arc<dyn Task + Send + Sync>) {
    TASKS.insert(name.to_string(), task);
}

/// 按优先级升序执行所有已注册的 `before` 钩子（应用启动前调用）。
pub async fn run_before_tasks() -> Result<()> {
    run_tasks(TaskType::Before).await
}

/// 按优先级降序执行所有已注册的 `after` 钩子（应用关闭后调用）。
pub async fn run_after_tasks() -> Result<()> {
    run_tasks(TaskType::After).await
}
