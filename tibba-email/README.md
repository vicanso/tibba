# tibba-email

**邮件发送**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

邮件发送服务抽象与 Resend HTTP API 实现，复用 `tibba-request` 出站客户端（无 `resend-rs` 冗余依赖），供邮箱验证与密码重置等场景使用。在 `dev` / `test` 环境下未配置 `api_key` 时自动降级为结构化日志输出，便于本地联调；生产环境未配置仍返回 `503 NotConfigured`。

## 依赖

依赖：`tibba-error`、`tibba-request`、`tibba-util`

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
