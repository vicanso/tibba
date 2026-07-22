# tibba-middleware

**HTTP 中间件**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

request_id、CORS、CSRF、限流、ETag、安全头、入口/统计、幂等等横切中间件与 MiddlewareOptions。

## 依赖

依赖：tibba-cache, tibba-error, tibba-session, tibba-state, tibba-util

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
