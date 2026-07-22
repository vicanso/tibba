# tibba-performance

**进程性能采样**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

采集 CPU / 内存 / 打开文件等进程指标，供健康检查与定时日志。

## 依赖

无内部依赖

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
