# tibba-router-common

**公共路由**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

健康检查、验证码、就绪探针、系统信息等公共 API 路由。

## 依赖

依赖：tibba-cache, tibba-error, tibba-performance, tibba-session, tibba-state, tibba-util

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
