# tibba-session

**会话**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

签名 Cookie + Redis Session，UserSession/AdminSession 提取器与权限判定。

## 依赖

依赖：tibba-cache, tibba-error, tibba-state, tibba-util

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
