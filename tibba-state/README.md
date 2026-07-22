# tibba-state

**应用与请求状态**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

进程级 `AppState`（并发计数、版本信息）与 task-local 请求上下文 `CTX`。

## 依赖

无内部依赖

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
