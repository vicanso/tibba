# tibba-sql

**PostgreSQL 连接池**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

从配置构建 sqlx PgPool，连接池统计与 URI 解析。

## 依赖

依赖：tibba-config, tibba-error, tibba-util

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
