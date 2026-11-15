# tibba-email

**邮件发送**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

邮件发送服务抽象与 Resend HTTP API 实现，复用 `tibba-request` 出站客户端（无 `resend-rs` 冗余依赖），供邮箱验证与密码重置等场景使用。

## 依赖

依赖：`tibba-error`、`tibba-request`

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
