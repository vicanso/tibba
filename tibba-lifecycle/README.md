# tibba-lifecycle

**生命周期钩子与定时任务**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

进程级任务编排，两者共享「全局具名注册表 + 统一驱动」形态：

| 模块 | 语义 | 注册 | 驱动 |
|------|------|------|------|
| `hook` | 启动前 / 关闭后各跑一次 | `register_task` | `run_before_tasks` / `run_after_tasks` |
| `scheduler` | 按 cron 周期性反复跑 | `register_job_task` | `run_scheduler_jobs` |

`hook` 的 before 阶段 **fail-fast**（避免半初始化状态对外服务），after 阶段
**best-effort**（错误仅记日志，确保所有清理都被尝试）。
`scheduler` 提供 `singleton_cron_job`，通过注入的分布式锁回调实现「全集群单实例」触发。

与 `tibba-job`（Postgres 异步任务队列）的分工：本 crate 是进程内编排，不落库、不跨进程重试。

## Features

| Feature | 默认 | 说明 |
|---------|------|------|
| `scheduler` | 关 | 启用 cron 定时任务。`tokio-cron-scheduler` 传递依赖较重，只用钩子的下游无需为其付出编译代价 |

```toml
# 只要启动/关闭钩子
tibba-lifecycle = "0.2.6"
# 同时要 cron 定时任务
tibba-lifecycle = { version = "0.2.6", features = ["scheduler"] }
```

## 日志

两个模块保留各自的 tracing target，便于分别排查：

```bash
RUST_LOG=tibba:hook=info        # 启停钩子
RUST_LOG=tibba:scheduler=info   # 定时任务
```

## 依赖

依赖：tibba-error

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `standard` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
