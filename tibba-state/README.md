# tibba-state

**应用与请求状态**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

进程级 `AppState`（并发计数、版本信息）、task-local 请求上下文 `CTX`，
以及进程资源采样（CPU / 内存 / 文件描述符 / 磁盘读写）。

## Features

| Feature | 默认 | 说明 |
|---------|------|------|
| `process-info` | 关 | 启用 `current_process_system_info` / `get_process_system_info` 进程采样。`sysinfo` 传递依赖较重，只用 `AppState` / `CTX` 的下游无需为其付出编译代价 |

```toml
# 只要 AppState / CTX
tibba-state = "0.2.6"
# 同时要进程指标采样
tibba-state = { version = "0.2.6", features = ["process-info"] }
```

## 依赖

无内部依赖

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
