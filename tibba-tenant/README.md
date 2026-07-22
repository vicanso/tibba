# tibba-tenant

**多租户原语**

> **分层**：扩展（Extension）— 可选能力，依赖核心后再发布

行级多租户：TenantId 提取器、从 Session 注入租户、SQL 过滤片段约定。

## 依赖

依赖：tibba-error, tibba-session

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
