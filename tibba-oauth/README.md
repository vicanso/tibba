# tibba-oauth

**OAuth 客户端**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

第三方 OAuth 登录客户端（GitHub / Google）与配置结构，复用 `tibba-request` 出站 HTTP 客户端完成令牌交换与用户信息拉取。

## 依赖

依赖：`tibba-error`、`tibba-request`

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
