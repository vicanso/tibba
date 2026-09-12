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

//! 进程级停机信号：一处安装信号处理，任意多处订阅。
//!
//! ## 解决什么
//! 在此之前，一个进程里并存着三套互不相干的停机机制：
//!
//! | 谁 | 机制 | 实际覆盖的信号 |
//! |----|------|---------------|
//! | HTTP | `axum::serve(..).with_graceful_shutdown(fut)` | SIGINT + SIGTERM |
//! | 任务队列 | `tibba-job` 自建 `watch::channel` + 显式 `shutdown()` | 靠调用方手动串 |
//! | Cron | `JobScheduler::shutdown_on_ctrl_c()` | **只有 SIGINT** |
//!
//! 第三条是个实打实的问题：那个方法内部就是 `tokio::signal::ctrl_c()`，而容器
//! 编排器（Docker / K8s）发的是 **SIGTERM**——也就是说容器部署下 cron 调度器
//! 从来不会优雅退出，正在执行的定时任务会被进程退出直接腰斩。
//!
//! 更普遍的问题是：`shutdown_signal()` 那种写法产出的是**一次性** future，谁先
//! 拿走谁独占。于是每新增一个后台循环，就得再发明一套自己的停机通知。
//!
//! 本模块把信号收敛成一个可任意克隆、可重复 await 的广播：
//!
//! ```ignore
//! // 进程启动时装一次
//! tibba_runtime::install_shutdown_signal();
//!
//! // HTTP 服务
//! axum::serve(listener, app)
//!     .with_graceful_shutdown(tibba_runtime::shutdown_token().cancelled_owned())
//!     .await?;
//!
//! // 任意后台循环
//! let shutdown = tibba_runtime::shutdown_token();
//! loop {
//!     tokio::select! {
//!         _ = shutdown.cancelled() => break,
//!         _ = tokio::time::sleep(interval) => do_work().await,
//!     }
//! }
//! ```
//!
//! ## 与 `hook` 的分工
//! 本模块负责「通知大家该停了」，`hook` 的 `run_after_tasks` 负责「大家停完之后
//! 做清理」。正确的顺序是：触发信号 → 各循环自行收尾 → 跑 after 钩子。

use crate::HOOK_LOG_TARGET;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::watch;
use tracing::{info, warn};

/// 停机信号源。持有它的一方负责触发；订阅方拿 [`ShutdownToken`]。
///
/// 通常不需要自己构造——进程级的那一个由本模块持有，用
/// [`shutdown_token`] / [`trigger_shutdown`] 访问。独立构造主要用于测试，
/// 以及需要「局部停机域」的场景（例如为一组任务单独控制生命周期）。
pub struct ShutdownSignal {
    tx: watch::Sender<bool>,
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownSignal {
    /// 创建一个尚未触发的停机信号。
    #[must_use]
    pub fn new() -> Self {
        let (tx, _) = watch::channel(false);
        Self { tx }
    }

    /// 取得一个订阅句柄，可任意克隆、跨任务传递。
    #[must_use]
    pub fn token(&self) -> ShutdownToken {
        ShutdownToken {
            rx: self.tx.subscribe(),
        }
    }

    /// 触发停机。**幂等**：重复触发不会产生额外效果。
    ///
    /// 触发后创建的 [`ShutdownToken`] 也会立刻处于已触发状态——否则在信号与
    /// 任务启动竞争时，晚一步启动的后台任务会永远等不到通知。
    pub fn trigger(&self) {
        // send_replace 即便当前没有订阅者也能写入状态，
        // 保证「先触发、后订阅」的顺序同样正确
        self.tx.send_replace(true);
    }

    /// 是否已触发。
    #[must_use]
    pub fn is_triggered(&self) -> bool {
        *self.tx.borrow()
    }
}

/// 停机信号的订阅句柄。
///
/// 可自由 `Clone`，每个副本都能独立 await，互不影响——这正是它与「一次性
/// future」的区别。
#[derive(Clone)]
pub struct ShutdownToken {
    rx: watch::Receiver<bool>,
}

impl ShutdownToken {
    /// 是否已经收到停机通知。适合在循环体内做快速检查。
    #[must_use]
    pub fn is_triggered(&self) -> bool {
        *self.rx.borrow()
    }

    /// 等待停机通知。已经触发时立即返回。
    ///
    /// 取 `&self` 而非 `&mut self`：内部克隆一份 receiver 来等待，因此同一个
    /// token 可以在 `tokio::select!` 里反复使用，不必为每轮循环重建。
    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        // 先查当前值：watch 的 `changed()` 只对**订阅之后**的变更返回，
        // 若信号在此之前就已触发，只等 changed() 会永远挂住
        if *rx.borrow() {
            return;
        }
        // 发送端与全局单例同寿，正常不会关闭；真关闭了也当作「该停了」
        let _ = rx.changed().await;
    }

    /// [`Self::cancelled`] 的 owned 版本，用于需要 `Future + 'static` 的接口
    /// （如 `axum::serve(..).with_graceful_shutdown(..)`）。
    pub async fn cancelled_owned(self) {
        self.cancelled().await;
    }
}

/// 进程级停机信号单例。
static SHUTDOWN: LazyLock<ShutdownSignal> = LazyLock::new(ShutdownSignal::new);

/// 信号处理是否已安装，保证 [`install_shutdown_signal`] 幂等。
static SIGNAL_INSTALLED: AtomicBool = AtomicBool::new(false);

/// 取得进程级停机信号的订阅句柄。
#[must_use]
pub fn shutdown_token() -> ShutdownToken {
    SHUTDOWN.token()
}

/// 手动触发进程级停机。
///
/// 供「非信号驱动」的退出路径使用：启动自检失败、致命错误、测试等。
pub fn trigger_shutdown() {
    SHUTDOWN.trigger();
}

/// 进程是否已进入停机流程。
#[must_use]
pub fn is_shutting_down() -> bool {
    SHUTDOWN.is_triggered()
}

/// 安装进程信号处理：收到 SIGINT 或 SIGTERM 时触发停机信号。
///
/// **幂等**，重复调用只会安装一次。须在 tokio runtime 内调用（内部 spawn 任务）。
///
/// # 为什么 SIGTERM 不能漏
/// 容器编排器停止容器时发的是 SIGTERM，只有手动 Ctrl-C 才是 SIGINT。只监听
/// SIGINT 的组件在生产环境里等于**没有**优雅停机——这正是此前 cron 调度器的
/// 状况（`JobScheduler::shutdown_on_ctrl_c` 内部只有 `tokio::signal::ctrl_c()`）。
///
/// 非 unix 平台没有 SIGTERM，只监听 Ctrl-C。
pub fn install_shutdown_signal() {
    // 已安装过则直接返回：重复 spawn 只会多出几个永远等不到第二次信号的任务
    if SIGNAL_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        let reason = wait_for_signal().await;
        info!(
            target: HOOK_LOG_TARGET,
            signal = reason,
            "shutdown signal received; notifying subscribers",
        );
        trigger_shutdown();
    });
}

/// 等待 SIGINT / SIGTERM，返回收到的信号名（仅用于日志）。
async fn wait_for_signal() -> &'static str {
    use tokio::signal;

    let ctrl_c = async {
        if signal::ctrl_c().await.is_err() {
            // 装不上处理器时退化为「永不触发」，而不是让进程直接崩掉：
            // 另一路信号仍可能正常工作
            warn!(target: HOOK_LOG_TARGET, "install SIGINT handler failed");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => {
                warn!(target: HOOK_LOG_TARGET, "install SIGTERM handler failed");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => "SIGINT",
        () = terminate => "SIGTERM",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    /// 测试一律用独立的 `ShutdownSignal`，不碰进程级单例——
    /// 触发是不可逆的，污染了全局就没法再测「未触发」的分支。
    #[tokio::test]
    async fn token_wakes_on_trigger() {
        let signal = ShutdownSignal::new();
        let token = signal.token();
        assert!(!token.is_triggered());

        let waiter = tokio::spawn(async move { token.cancelled().await });
        // 给等待方一点时间真正挂起，确保测的是「唤醒」而不是「早已触发」
        tokio::time::sleep(Duration::from_millis(10)).await;
        signal.trigger();

        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("触发后必须及时唤醒")
            .expect("等待任务不应 panic");
    }

    /// **关键不变量**：先触发、后订阅，同样要能立刻返回。
    ///
    /// 否则信号与任务启动存在竞争时，晚一步 spawn 的后台任务会永远挂住，
    /// 把进程拖到被 SIGKILL。
    #[tokio::test]
    async fn token_created_after_trigger_is_already_cancelled() {
        let signal = ShutdownSignal::new();
        signal.trigger();

        let token = signal.token();
        assert!(token.is_triggered());
        tokio::time::timeout(Duration::from_millis(100), token.cancelled())
            .await
            .expect("已触发的信号不得让订阅方等待");
    }

    /// 多个订阅者必须都被唤醒，且 `cancelled()` 取 `&self`，可在循环里反复使用。
    #[tokio::test]
    async fn all_subscribers_wake_and_token_is_reusable() {
        let signal = ShutdownSignal::new();
        let waiters: Vec<_> = (0..4)
            .map(|_| {
                let token = signal.token();
                tokio::spawn(async move {
                    token.cancelled().await;
                    // 同一个 token 再等一次仍应立即返回
                    token.cancelled().await;
                    true
                })
            })
            .collect();

        tokio::time::sleep(Duration::from_millis(10)).await;
        signal.trigger();

        for waiter in waiters {
            let done = tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .expect("所有订阅者都应被唤醒")
                .expect("等待任务不应 panic");
            assert!(done);
        }
    }

    /// 克隆出的 token 与原件等价。
    #[tokio::test]
    async fn cloned_token_shares_the_signal() {
        let signal = ShutdownSignal::new();
        let token = signal.token();
        let cloned = token.clone();
        signal.trigger();

        assert!(cloned.is_triggered());
        tokio::time::timeout(Duration::from_millis(100), cloned.cancelled_owned())
            .await
            .expect("克隆件应当同样已触发");
    }

    /// 触发是幂等的。
    #[test]
    fn trigger_is_idempotent() {
        let signal = ShutdownSignal::new();
        assert!(!signal.is_triggered());
        signal.trigger();
        signal.trigger();
        assert_eq!(signal.is_triggered(), true);
    }

    /// 进程级单例的读取路径可用（不触发，避免污染其它测试）。
    #[test]
    fn process_token_is_available_and_starts_untriggered() {
        assert!(!is_shutting_down());
        assert!(!shutdown_token().is_triggered());
    }
}
