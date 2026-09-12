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

//! 启动 / 关闭钩子：全局注册表 + 按优先级驱动执行。

use crate::HOOK_LOG_TARGET;
use dashmap::DashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tibba_error::Error;
use tokio::time::timeout;
use tracing::{error, info, warn};

type Result<T> = std::result::Result<T, Error>;

/// 关闭阶段所有 `after` 钩子的总预算。
///
/// 取 10s 是为了留在常见的 K8s `terminationGracePeriodSeconds`（默认 30s）之内，
/// 让进程有机会自己退干净，而不是被 SIGKILL 砍掉。
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// 单个 `after` 钩子的时间上限，防止某一个钩子吃掉全部预算。
pub const DEFAULT_TASK_TIMEOUT: Duration = Duration::from_secs(5);

/// 关闭阶段的超时预算。
///
/// 两级限制缺一不可：只有单任务上限时，N 个任务最坏仍要 N×T；只有总预算时，
/// 第一个卡住的任务会吃光预算，后面的清理一个都跑不到。
#[derive(Debug, Clone, Copy)]
pub struct ShutdownTimeouts {
    /// 所有 `after` 钩子合计的时间上限
    total: Duration,
    /// 单个 `after` 钩子的时间上限
    per_task: Duration,
}

impl Default for ShutdownTimeouts {
    fn default() -> Self {
        Self {
            total: DEFAULT_SHUTDOWN_TIMEOUT,
            per_task: DEFAULT_TASK_TIMEOUT,
        }
    }
}

impl ShutdownTimeouts {
    /// 使用默认预算，见 [`DEFAULT_SHUTDOWN_TIMEOUT`] / [`DEFAULT_TASK_TIMEOUT`]。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置所有 `after` 钩子合计的时间上限，支持链式调用。
    #[must_use]
    pub fn with_total(mut self, total: Duration) -> Self {
        self.total = total;
        self
    }

    /// 设置单个 `after` 钩子的时间上限，支持链式调用。
    #[must_use]
    pub fn with_per_task(mut self, per_task: Duration) -> Self {
        self.per_task = per_task;
        self
    }
}

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

/// 注册序号发生器，用于在优先级相同时给出确定的执行次序。
static REGISTRATION_SEQ: AtomicU64 = AtomicU64::new(0);

/// 全局任务注册表，键为任务名称，值为 `(注册序号, 任务实例)`。
///
/// 之所以要存注册序号：`DashMap` 的迭代顺序不确定，而排序只按 `priority`——
/// 于是**同优先级**任务的执行次序每次进程启动都可能不同。启动钩子之间经常存在
/// 隐式依赖（先建连接池、再预热缓存），这种不确定性会变成「偶发启动失败」，
/// 且本地几乎复现不出来。
static TASKS: LazyLock<DashMap<String, RegisteredTask>> = LazyLock::new(DashMap::new);

/// 注册表里的一项：注册序号 + 任务实例。
type RegisteredTask = (u64, Arc<dyn Task>);

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
///
/// 先整体收集再排序，**不**在持有 `DashMap` 分片锁的状态下执行任务。
///
/// 排序键是 `(优先级, 注册序号)`：优先级相同时按注册先后决定，保证每次启动
/// 的执行次序完全一致。`after` 阶段两者都取反，使其严格是 `before` 的逆序——
/// 「后初始化的先清理」是资源释放的正确方向（例如先关连接池的使用者，再关池）。
fn collect_sorted(task_type: TaskType) -> Vec<(String, Arc<dyn Task>)> {
    let mut tasks: Vec<(u64, String, Arc<dyn Task>)> = TASKS
        .iter()
        .map(|item| {
            let (seq, task) = item.value();
            (*seq, item.key().clone(), task.clone())
        })
        .collect();

    // 用 i64 承载以便取负数实现降序：u8 / u64 无法直接配合 sort_by_key
    tasks.sort_by_key(|(seq, _, task)| {
        let p = i64::from(task.priority());
        let seq = *seq as i64;
        match task_type {
            TaskType::Before => (p, seq),
            TaskType::After => (-p, -seq),
        }
    });
    tasks
        .into_iter()
        .map(|(_, name, task)| (name, task))
        .collect()
}

/// 按优先级顺序执行所有已注册的钩子任务。
/// - Before 阶段 fail-fast：首个错误立即返回，跳过剩余任务（避免半初始化的启动状态）
/// - After 阶段 best-effort：每个错误记日志后继续，确保所有清理任务都被尝试，最终始终返回 Ok
///
/// `timeouts` 仅对 After 阶段生效（Before 传 `None`）：超时的钩子会被**丢弃**
/// （future 被 drop，清理动作中途取消），随后继续跑下一个。这是刻意的取舍——
/// 关闭阶段拖着不退的代价是被 SIGKILL，那样一个清理都做不成。
async fn run_tasks(task_type: TaskType, timeouts: Option<ShutdownTimeouts>) -> Result<()> {
    let deadline = timeouts.map(|t| Instant::now() + t.total);

    for (name, task) in collect_sorted(task_type) {
        let start = Instant::now();

        // 本任务可用的时间片 = 单任务上限与总预算剩余量取小
        let budget = match (timeouts, deadline) {
            (Some(t), Some(end)) => {
                let remaining = end.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    warn!(
                        target: HOOK_LOG_TARGET,
                        task_type = task_type.label(),
                        name,
                        "shutdown budget exhausted; remaining tasks skipped",
                    );
                    break;
                }
                Some(t.per_task.min(remaining))
            }
            _ => None,
        };

        let outcome = match (task_type, budget) {
            (TaskType::Before, _) => task.before().await,
            (TaskType::After, None) => task.after().await,
            (TaskType::After, Some(limit)) => match timeout(limit, task.after()).await {
                Ok(result) => result,
                Err(_) => {
                    error!(
                        target: HOOK_LOG_TARGET,
                        task_type = task_type.label(),
                        name,
                        timeout_ms = limit.as_millis(),
                        "after task timed out; abandoned",
                    );
                    // best-effort：放弃这个，继续尝试后续清理
                    continue;
                }
            },
        };

        match outcome {
            Ok(executed) => {
                if executed {
                    info!(
                        target: HOOK_LOG_TARGET,
                        task_type = task_type.label(),
                        name,
                        elapsed = start.elapsed().as_millis(),
                    );
                }
            }
            Err(err) => {
                error!(
                    target: HOOK_LOG_TARGET,
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

/// 注册一个具名钩子任务。同名任务重复注册时，新任务会覆盖旧任务
/// （并取得一个新的注册序号，即排到同优先级的末尾）。
pub fn register_task(name: impl Into<String>, task: Arc<dyn Task>) {
    let seq = REGISTRATION_SEQ.fetch_add(1, Ordering::Relaxed);
    TASKS.insert(name.into(), (seq, task));
}

/// 按优先级升序执行所有已注册的 `before` 钩子（应用启动前调用）。
/// 任一任务返回错误时立即停止并向上传播。
///
/// 注意本阶段**没有超时**：启动钩子卡住会让应用一直起不来，但那是可见的
/// （readiness 探针不通过，编排系统会重启），不像关闭阶段那样悄悄挂死。
pub async fn run_before_tasks() -> Result<()> {
    run_tasks(TaskType::Before, None).await
}

/// 按优先级降序执行所有已注册的 `after` 钩子（应用关闭后调用），使用默认超时预算。
///
/// 任务错误仅记日志，所有任务都会被尝试；本函数始终返回 `Ok(())`。
/// 超时预算见 [`ShutdownTimeouts`]，需要自定义时用 [`run_after_tasks_with`]。
pub async fn run_after_tasks() -> Result<()> {
    run_after_tasks_with(ShutdownTimeouts::default()).await
}

/// 同 [`run_after_tasks`]，但使用自定义的超时预算。
///
/// 无论钩子如何卡住，本函数都会在 `timeouts.total` 内返回——这是优雅关闭
/// 能够成立的前提：超过编排系统的宽限期就只剩 SIGKILL，一个清理都做不完。
pub async fn run_after_tasks_with(timeouts: ShutdownTimeouts) -> Result<()> {
    run_tasks(TaskType::After, Some(timeouts)).await
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

    /// **回归守卫**：同优先级任务必须按注册先后执行，且每次都一样。
    ///
    /// 此前排序只看 `priority`，次序取决于 `DashMap` 的迭代顺序——同优先级
    /// 的启动钩子每次进程启动都可能换个顺序跑，隐式依赖会变成偶发启动失败。
    #[tokio::test]
    async fn equal_priority_follows_registration_order() {
        let _g = serial();
        // 多跑几轮：顺序若依赖哈希迭代，重复运行会暴露出来
        for _ in 0..8 {
            reset();
            let trace: Trace = Arc::new(Mutex::new(Vec::new()));
            for name in ["first", "second", "third", "fourth"] {
                register_task(
                    name,
                    Arc::new(ProbeTask {
                        name,
                        priority: 7,
                        before_result: ok_true,
                        after_result: ok_true,
                        trace: trace.clone(),
                    }),
                );
            }

            run_before_tasks().await.unwrap();
            assert_eq!(
                &*trace.lock().unwrap(),
                &["first", "second", "third", "fourth"]
            );
        }
    }

    /// `after` 必须是 `before` 的严格逆序：后初始化的先清理。
    #[tokio::test]
    async fn after_reverses_registration_order_within_same_priority() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));
        for name in ["pool", "cache", "warmup"] {
            register_task(
                name,
                Arc::new(ProbeTask {
                    name,
                    priority: 0,
                    before_result: ok_true,
                    after_result: ok_true,
                    trace: trace.clone(),
                }),
            );
        }

        run_after_tasks().await.unwrap();
        assert_eq!(&*trace.lock().unwrap(), &["warmup", "cache", "pool"]);
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

    /// 永远不返回的 after 钩子，模拟「等一个已断开的连接」。
    struct HangingTask {
        priority: u8,
    }

    impl Task for HangingTask {
        fn priority(&self) -> u8 {
            self.priority
        }
        fn after(&self) -> BoxFuture<'_, Result<bool>> {
            Box::pin(async {
                // 远超测试设置的超时；若超时机制失效，测试会挂住而非通过
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Ok(true)
            })
        }
    }

    /// 卡住的钩子必须被放弃，且**不能**阻断后续清理任务。
    #[tokio::test]
    async fn hung_after_task_is_abandoned_and_others_still_run() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));

        // after 阶段按优先级降序：先跑卡住的，再跑正常的
        register_task("hung", Arc::new(HangingTask { priority: 100 }));
        register_task(
            "cleanup",
            Arc::new(ProbeTask {
                name: "cleanup",
                priority: 10,
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );

        let started = Instant::now();
        run_after_tasks_with(
            ShutdownTimeouts::new()
                .with_per_task(Duration::from_millis(50))
                .with_total(Duration::from_secs(5)),
        )
        .await
        .unwrap();

        assert_eq!(
            &*trace.lock().unwrap(),
            &["cleanup"],
            "卡住的钩子应被放弃，后续清理必须照常执行"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "不得等待卡住的钩子，实际耗时 {:?}",
            started.elapsed()
        );
    }

    /// 总预算耗尽后，剩余任务整体跳过——保证整个关闭阶段有硬上限。
    #[tokio::test]
    async fn total_budget_caps_whole_shutdown() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));

        // 三个都卡住，单任务上限 50ms，总预算 80ms → 跑完前两个就该耗尽
        for (i, name) in ["h1", "h2", "h3"].iter().enumerate() {
            register_task(
                *name,
                Arc::new(HangingTask {
                    priority: (100 - i) as u8,
                }),
            );
        }
        register_task(
            "last",
            Arc::new(ProbeTask {
                name: "last",
                priority: 0, // after 降序 → 最后跑
                before_result: ok_true,
                after_result: ok_true,
                trace: trace.clone(),
            }),
        );

        let started = Instant::now();
        run_after_tasks_with(
            ShutdownTimeouts::new()
                .with_per_task(Duration::from_millis(50))
                .with_total(Duration::from_millis(80)),
        )
        .await
        .unwrap();

        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "总预算 80ms，实际耗时 {elapsed:?}"
        );
        // 预算被前面卡住的任务吃光，最后的清理没机会跑——这是硬上限的代价，
        // 也正因如此 per_task 必须设得足够小
        assert!(trace.lock().unwrap().is_empty());
    }

    /// 正常（不卡）的 after 钩子不受超时影响，全部照常执行。
    #[tokio::test]
    async fn healthy_tasks_are_unaffected_by_timeouts() {
        let _g = serial();
        reset();
        let trace: Trace = Arc::new(Mutex::new(Vec::new()));
        for (name, priority) in [("a", 50u8), ("b", 10u8)] {
            register_task(
                name,
                Arc::new(ProbeTask {
                    name,
                    priority,
                    before_result: ok_true,
                    after_result: ok_true,
                    trace: trace.clone(),
                }),
            );
        }

        run_after_tasks().await.unwrap();
        assert_eq!(&*trace.lock().unwrap(), &["a", "b"]);
    }
}
