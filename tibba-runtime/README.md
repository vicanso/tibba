# tibba-runtime

**进程运行时：状态、资源与生命周期**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

三部分都描述「正在运行的这个进程」，与请求处理、存储访问无关：

| 模块 | 内容 | feature |
|------|------|---------|
| `app_state` | `AppState`：服务标识、运行标志、并发计数（流控用）、启动时间 | 始终编译 |
| `ctx` | task-local 请求上下文 `CTX` / `Context`（设备 ID、trace ID、耗时、登录账号） | 始终编译 |
| `process` | 进程 CPU / 内存 / 文件描述符 / 磁盘读写采样 | `process-info` |
| `hook` | 启动前 / 关闭后钩子 | 始终编译 |
| `scheduler` | cron 定时任务 | `scheduler` |

`hook` 与 `scheduler` 共享同一形态——全局具名注册表 + 统一驱动：

| 语义 | 注册 | 驱动 |
|------|------|------|
| 启动前 / 关闭后各跑一次 | `register_task` | `run_before_tasks` / `run_after_tasks` |
| 按 cron 周期性反复跑 | `register_job_task` | `run_scheduler_jobs` |

`hook` 的 before 阶段 **fail-fast**（避免半初始化状态对外服务），after 阶段
**best-effort**（错误仅记日志，确保所有清理都被尝试）。
`scheduler` 提供 `singleton_cron_job`，通过注入的分布式锁回调实现「全集群单实例」触发。

与 `tibba-job`（Postgres 异步任务队列）的分工：本 crate 是进程内编排，不落库、不跨进程重试。

## Features

两个可选 feature 默认关闭：重依赖不会流向只用轻量部分的下游。

| Feature | 默认 | 引入 | 说明 |
|---------|------|------|------|
| `process-info` | 关 | `sysinfo`, `cached` | `current_process_system_info` / `get_process_system_info` |
| `scheduler` | 关 | `tokio-cron-scheduler` | `Job`, `register_job_task`, `run_scheduler_jobs`, `singleton_cron_job` |

```toml
# 只要 AppState / CTX / 启停钩子
tibba-runtime = "0.2.6"
# 全部能力
tibba-runtime = { version = "0.2.6", features = ["process-info", "scheduler"] }
```

## 日志

钩子与调度器保留各自的 tracing target，便于分别排查：

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
