# tibba-model-builtin

**内置业务模型**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

文件、用户 OAuth 关联、权限、HTTP 探测等内置表模型（脚手架默认依赖）。

## 依赖

依赖：tibba-error, tibba-model

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
