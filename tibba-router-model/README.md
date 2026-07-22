# tibba-router-model

**通用 Model 路由**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

schema 驱动的动态 CRUD，支持按模型注册权限码。

## 依赖

依赖：tibba-error, tibba-hook, tibba-model, tibba-session, tibba-util, tibba-validator

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
