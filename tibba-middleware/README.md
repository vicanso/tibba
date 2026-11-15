# tibba-middleware

**HTTP 中间件与错误国际化**

> **分层**：标准（Standard）— 合并原 `tibba-middleware` 与 `tibba-i18n`

提供标准 HTTP 横切中间件与一键装配工具 `apply_middleware_stack`：

- 基础层：`request_id`、`security_headers`、`cors`、压缩、超时错误处理、`entry`、`stats`、`processing_limit`、`wait`、`tracker`、`redis_ip_rate_limit`
- 协议/安全层：`validate_csrf` / `csrf_token`、`http_cache`、`idempotency`
- 错误本地化：`i18n` 模块（`Catalog`、`init`、`i18n` 中间件）
- 可选 feature：`otel`（`otel_trace` W3C 分布式追踪上下文传播）

## 依赖

依赖：`tibba-auth`、`tibba-cache`、`tibba-error`、`tibba-runtime`、`tibba-util`

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
