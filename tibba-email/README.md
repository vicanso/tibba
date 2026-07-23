# tibba-email

**邮件发送**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

邮件服务抽象，默认 Resend 实现，供验证邮件/密码重置等使用。

## 依赖

依赖：tibba-config, tibba-error

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
