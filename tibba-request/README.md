# tibba-request

**出站 HTTP / SSE 客户端**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

基于 `Arc` 共享只读配置（可低成本 `Clone` 复用连接池）的链式 `ClientBuilder` / `Client` 与 `Params` 构造器：

- **可靠性与流控**：默认超时（30s 总超时 / 5s 建连超时，支持 `.without_timeout()`）、并发上限 `with_max_processing`（RAII 计数守卫，超限快速返回 503 `Error::ConcurrencyLimit`）、熔断器 `with_circuit_breaker`（503 `Error::CircuitOpen`）、幂等重试 `with_retry`、响应体积上限 `with_max_response_bytes`（默认 `DEFAULT_MAX_RESPONSE_BYTES` = 64 MiB）
- **安全与脱敏**：SSRF 防护 `with_deny_internal_targets`（默认禁用环境变量代理 `.no_proxy()`、解析校验 IPv4/IPv6 私网/保留网段与 `dns_overrides`、拒绝 3xx 重定向并截断汇报 `Location`）；`Error::Request` / `Error::Build` 转 `tibba_error::Error` 时剥离 URL 避免 query key 泄漏；`ClientConfig` / `ClientBuilder` / `Params` 脱敏 `Debug`
- **请求构造与执行**：`Params` 支持 `with_header` / `with_bearer_auth` / `with_form` / `with_bytes` / `with_json_bytes`；`Client` 提供 `execute`（返回 `RawResponse`）、`request_raw`、`post_form` 及常用 JSON 方法，并支持 `HttpInterceptor` 与 W3C `traceparent`（OTel）注入

## Features

| Feature | 默认 | 内容 |
|---------|------|------|
| `stream` | 关 | 启用 `reqwest/stream` + `futures`，导出 `Client::execute_stream`（并发守卫持有至流结束）与 UTF-8 安全的 SSE 行级读取器 `SseReader` / `SsePoll`（默认单行上限 `DEFAULT_MAX_SSE_LINE_BYTES` = 16 MiB） |
| `tls-info` | 关 | 启用 `x509-parser`，采集对端 TLS 证书有效期到 `HttpStats` |
| `otel` | 关 | 启用 `opentelemetry` + `tracing-opentelemetry`，出站请求自动注入 W3C `traceparent` / `tracestate` |

## 依赖

依赖：tibba-error, tibba-util

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
