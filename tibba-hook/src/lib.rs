// Copyright 2026 Tree xie.
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
use tracing::{error, info};

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:hook=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:hook";

type Result<T> = std::result::Result<T, Error>;

/// 装箱的异步 Future，用于 trait object 场景下的异步方法返回类型。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 生命周期钩子 trait，用于在应用启动/关闭时执行自定义逻辑。
///
/// 由于实例以 `Arc<dyn Task>` 存入全局注册表并跨线程使用，要求实现类型必须
/// 满足 `Send + Sync`——把约束直接写在 trait 上，存储类型就是简洁的
/// `Arc<dyn Task>`，不必到处复述 `+ Send + Sync`。
///
/// - `before`：应用启动前执行（如初始化资源），按优先级从低到高顺序调用，**fail-fast**。
/// - `after`：应用关闭后执行（如释放资源），按优先级从高到低顺序调用，**best-effort**
///   （任一任务出错只记日志、继续执行后续清理，确保资源被尽可能释放）。
/// - 返回 `true` 表示该钩子实际执行了操作，会记录耗时日志；返回 `false` 则静默跳过。
pub trait Task: Send + Sync {
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
static TASKS: LazyLock<DashMap<String, Arc<dyn Task>>> = LazyLock::new(DashMap::new);

/// 任务执行阶段：启动前（Before）或关闭后（After）。
#[derive(Clone, Copy)]
enum TaskType {
    Before,
    After,
}

impl TaskType {
    /// 仅用于日志输出的短标签。
    fn label(self) -> &'static str {
        match self {
            TaskType::Before => "before",
            TaskType::After => "after",
        }
    }
}

/// 收集所有已注册任务并按当前阶段所需顺序排序。
fn collect_sorted(task_type: TaskType) -> Vec<(String, Arc<dyn Task>)> {
    let mut tasks: Vec<(String, Arc<dyn Task>)> = TASKS
        .iter()
        .map(|item| (item.key().clone(), item.value().clone()))
        .collect();

    // 用 i16 承载 priority 以便取负数实现降序，u8 无法直接配合 sort_by_key + Reverse
    tasks.sort_by_key(|(_, task)| {
        let p = task.priority() as i16;
        match task_type {
            TaskType::Before => p,
            TaskType::After => -p,
        }
    });
    tasks
}

/// 按优先级顺序执行所有已注册的钩子任务。
/// - Before 阶段 fail-fast：首个错误立即返回，跳过剩余任务（避免半初始化的启动状态）
/// - After 阶段 best-effort：每个错误记日志后继续，确保所有清理任务都被尝试，最终始终返回 Ok
async fn run_tasks(task_type: TaskType) -> Result<()> {
    for (name, task) in collect_sorted(task_type) {
        let start = std::time::Instant::now();
        let outcome = match task_type {
            TaskType::Before => task.before().await,
            TaskType::After => task.after().await,
        };

        match outcome {
            Ok(executed) => {
                if executed {
                    info!(
                        target: LOG_TARGET,
                        task_type = task_type.label(),
                        name,
                        elapsed = start.elapsed().as_millis(),
                    );
                }
            }
            Err(err) => {
                error!(
                    target: LOG_TARGET,
                    task_type = task_type.label(),
                    name,
                    elapsed = start.elapsed().as_millis(),
                    error = %err,
                );
                if matches!(task_type, TaskType::Before) {
                    // 启动期 fail-fast，避免应用以半初始化状态对外提供服务
                    return Err(err);
                }
                // After 阶段继续执行后续清理任务
            }
        }
    }
    Ok(())
}

/// 注册一个具名钩子任务。同名任务重复注册时，新任务会覆盖旧任务。
pub fn register_task(name: impl Into<String>, task: Arc<dyn Task>) {
    TASKS.insert(name.into(), task);
}

/// 按优先级升序执行所有已注册的 `before` 钩子（应用启动前调用）。
/// 任一任务返回错误时立即停止并向上传播。
pub async fn run_before_tasks() -> Result<()> {
    run_tasks(TaskType::Before).await
}

/// 按优先级降序执行所有已注册的 `after` 钩子（应用关闭后调用）。
/// 任务错误仅记日志，所有任务都会被执行；本函数始终返回 `Ok(())`。
pub async fn run_after_tasks() -> Result<()> {
    run_tasks(TaskType::After).await
}

#[cfg(test)]
// 测试通过 std::sync::Mutex 串行化共享的全局 TASKS 注册表，guard 跨 await
// 持有；每个 #[tokio::test] 跑在独占的单线程 runtime 上，不存在跨任务的死锁
// 风险，因此放行 await_holding_lock。
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;

    /// 共享执行轨迹，用于断言任务执行顺序与是否被调用过。
    type Trace = Arc<Mutex<Vec<&'static str>>>;

    /// 通用测试任务：可配置名称、优先级以及 before/after 的返回值。
    struct ProbeTask {
        name: &'static str,
        priority: u8,
        before_result: fn() -> Result<bool>,
        after_result: fn() -> Result<bool>,
        trace: Trace,
    }

    impl Task for ProbeTask {
        fn priority(&self) -> u8 {
            self.priority
        }
        fn before(&self) -> BoxFuture<'_, Result<bool>> {
            let name = self.name;
            let trace = self.trace.clone();
            let f = self.before_result;
            Box::pin(async move {
                trace.lock().unwrap().push(name);
                f()
            })
        }
        fn after(&self) -> BoxFuture<'_, Result<bool>> {
            let name = self.name;
            let trace = self.trace.clone();
            let f = self.after_result;
            Box::pin(async move {
                trace.lock().unwrap().push(name);
                f()
            })
        }
    }

    /// 清空全局注册表。测试通过 serial mutex 串行化以避免相互干扰。
    fn reset() {
        TASKS.clear();
    }

    /// 全局串行锁：注册表是单例，多个并发测试会污染彼此的注册项。
    static SERIAL: Mutex<()> = Mutex::new(());

    /// 取锁（PoisonError 时仍取回 guard），保证一次只跑一个测试。
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn ok_true() -> Result<bool> {
        Ok(true)
    }
    fn boom() -> Result<bool> {
        Err(Error::new("boom"))
    }

    #[tokio::test]
    async fn before_runs_in_ascending_priority_order() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));
        register_task(
            "high-prio",
            Arc::new(ProbeTask {
                name: "high",
                priority: 1,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );
        register_task(
            "low-prio",
            Arc::new(ProbeTask {
                name: "low",
                priority: 200,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );

        run_before_tasks().await.unwrap();
        assert_eq!(&*trace.lock().unwrap(), &["high", "low"]);
    }

    #[tokio::test]
    async fn after_runs_in_descending_priority_order() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));
        register_task(
            "a",
            Arc::new(ProbeTask {
                name: "a",
                priority: 10,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );
        register_task(
            "b",
            Arc::new(ProbeTask {
                name: "b",
                priority: 50,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );

        run_after_tasks().await.unwrap();
        // priority=50 先于 priority=10
        assert_eq!(&*trace.lock().unwrap(), &["b", "a"]);
    }

    #[tokio::test]
    async fn before_is_fail_fast_on_first_error() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));
        register_task(
            "first",
            Arc::new(ProbeTask {
                name: "first",
                priority: 0,
                before_result: boom,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );
        register_task(
            "second",
            Arc::new(ProbeTask {
                name: "second",
                priority: 10,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );

        let err = run_before_tasks().await.unwrap_err();
        assert!(err.to_string().contains("boom"));
        // 首个失败后第二个任务不应被执行
        assert_eq!(&*trace.lock().unwrap(), &["first"]);
    }

    #[tokio::test]
    async fn after_is_best_effort_continues_past_errors() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));
        register_task(
            "first",
            Arc::new(ProbeTask {
                name: "first",
                priority: 100, // 先跑（after 降序）
                before_result: ok_true,
                after_result: boom,
                trace: trace.clone(),
            }),
        );
        register_task(
            "second",
            Arc::new(ProbeTask {
                name: "second",
                priority: 10,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );

        // After 即便首个出错也应返回 Ok，并执行后续任务
        run_after_tasks().await.unwrap();
        assert_eq!(&*trace.lock().unwrap(), &["first", "second"]);
    }

    #[tokio::test]
    async fn register_task_overwrites_same_name() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));
        register_task(
            "dup",
            Arc::new(ProbeTask {
                name: "v1",
                priority: 0,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );
        register_task(
            "dup",
            Arc::new(ProbeTask {
                name: "v2",
                priority: 0,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );

        run_before_tasks().await.unwrap();
        assert_eq!(&*trace.lock().unwrap(), &["v2"]);
    }

    #[tokio::test]
    async fn empty_registry_is_ok() {
        let _g = serial();
        reset();
        run_before_tasks().await.unwrap();
        run_after_tasks().await.unwrap();
    }
}
