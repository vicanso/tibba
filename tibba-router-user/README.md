# tibba-router-user

**用户路由**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

用户注册登录、Session 轮换/销毁、JWT 签发与刷新令牌轮换、API Key、OAuth（nonce 绑定与 2FA 门控）、TOTP 锁定/防重放、邮箱验证、密码重置与路由级 IP 限流。

## 依赖

依赖：`tibba-cache`、`tibba-crypto`、`tibba-email`、`tibba-error`、`tibba-jwt`、`tibba-middleware`、`tibba-model`、`tibba-model-builtin`、`tibba-oauth`、`tibba-session`、`tibba-totp`、`tibba-util`

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
