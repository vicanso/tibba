# tibba-request

**出站 HTTP 客户端**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

链式 ClientBuilder：超时默认值、拦截器、重试（幂等语义）、熔断、OTel 注入、SSRF 防护。

## 依赖

依赖：tibba-error, tibba-util

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
