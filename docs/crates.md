# tibba crate 分层

Workspace 内共 **12 个**可发布的 `tibba-*` crate，精简为 **核心（Core, 7）** 与 **标准（Standard, 5）** 两层：

1. 明确依赖边界，Core 不反向依赖 Standard；
2. 无重依赖隐式拖累，可选重能力通过 feature flag 隔离；
3. 按严格拓扑 DAG 分批发布到 crates.io（见 [publish.sh](../scripts/publish.sh)）。

版本号与 Rust 版本要求统一见根 [Cargo.toml](../Cargo.toml) 的 `[workspace.package]`，依赖拓扑图见 [modules.md](modules.md)。派生新项目见 [scaffold.md](scaffold.md)。

## 核心 Core（7 crates）

最小服务底座，覆盖错误模型、工具与校验、配置、密码原语、运行态、缓存与出站 HTTP。

| Crate | 职责 | 可选 feature |
|-------|------|--------------|
| `tibba-error` | HTTP 错误类型（分类、状态码、敏感信息脱敏、上游重建） | |
| `tibba-util` | 通用工具、日期时间、`validator` 校验器（`x_*`）、安全解压与签名辅助 | `compression`、`http`（默认全开） |
| `tibba-config` | 多层配置加载与校验（支持 `*_FILE` 密钥文件展开） | |
| `tibba-crypto` | Argon2id 密码哈希（支持全局 pepper）、HMAC 签名与 AES-256-GCM 对称加密 | |
| `tibba-runtime` | `AppState`、请求上下文、优雅停机信号、进程采样与 cron 调度 | `process-info`、`scheduler` |
| `tibba-cache` | Redis 连接池与缓存、分布式锁、限流计数、两级本地缓存 + 失效广播 | |
| `tibba-request` | 出站 HTTP / SSE 客户端（并发限制、熔断、重试、SSRF 防护、OTel 传播）与可选邮件发送 | `stream`、`tls-info`、`otel`、`email` |

## 标准 Standard（5 crates）

面向 Web/REST 应用的标准业务与横切构件，仅依赖 Core 与下层 Standard crate。

| Crate | 职责 | 可选 feature |
|-------|------|--------------|
| `tibba-model` | Postgres 连接池（`sql`）、`Model` 抽象、通用分页/排序、基础表模型与可选异步任务队列（`job`） | `job` |
| `tibba-opendal` | 对象存储抽象（路径校验与凭证脱敏；默认本地文件系统） | `fs`（默认开）、`s3`、`http` |
| `tibba-auth` | Cookie + Redis 会话、RBAC 权限判定与可选鉴权扩展 | `jwt`、`totp`、`oauth`、`oauth-google`、`oauth-github` |
| `tibba-middleware` | 统一 HTTP 中间件栈装配、`request_id`、CORS、CSRF、ETag、限流、幂等与 i18n 本地化 | `otel` |
| `tibba-router` | HTTP 路由聚合（`common`、`configuration`、`user`、`model`、`user_admin`、`rbac`、`audit`、可选 `email` / `file` / `job` / `sql-query`）及 OpenAPI 拼接 | `email`、`file`、`job`、`jwt`、`totp`、`oauth`、`api-key`、`process-info`、`sql-query` |

## 主应用 Feature 映射

根包 [Cargo.toml](../Cargo.toml) 默认仅启用 `default = ["file", "job", "email"]`：

| 根 Feature | 启用链路 |
|------------|----------|
| `file` | `tibba-opendal` + `tibba-router/file`（`src/router.rs` 挂载 `/files/*`，`AppCtx` 初始化 `Storage`） |
| `s3` | `file` + `tibba-opendal/s3` |
| `job` | `tibba-model/job` + `tibba-router/job`（`src/main.rs` 启动 worker，`src/router.rs` 挂载 `/jobs/*`） |
| `email` | `tibba-request/email` + `tibba-router/email`（启用 `EmailConfig`、`/users/email/*` 与 `/users/password/reset/*`） |
| `jwt` | `tibba-auth/jwt` + `tibba-router/jwt` |
| `totp` | `tibba-auth/totp` + `tibba-router/totp` |
| `oauth` | `tibba-auth/oauth` + `tibba-router/oauth` |
| `api-key` | `tibba-router/api-key`（挂接 `api_key_auth` 到 Session 之后） |
| `otel` | `opentelemetry` + `tibba-middleware/otel` + `tibba-request/otel` |
| `process-info` | `tibba-runtime/process-info` + `tibba-router/process-info` |
| `scheduler` | `tibba-runtime/scheduler`（`src/app_ctx.rs` 注册周期健康检查与指标任务） |
| `embed-admin` | `rust-embed` + `mime_guess2`（内嵌 `admin/dist/` 静态资源） |
| `sql-query` | `tibba-router/sql-query`（`/queries/{group}/{name}` 只读 SQL 查询与 `/sql-queries` 超管管理） |
| `app-auth` | `tibba-router/app-auth`（全局 `app_id + ts + sign` 签名鉴权中间件与 `/apps` 应用授权管理） |
| `full` | 打开以上全部 14 项 |

## 发布

```bash
# 仅核心（7 crates，按 DAG 分 3 批）
./scripts/publish.sh core

# 仅标准（5 crates，按 DAG 分 4 批，需核心已在 crates.io）
./scripts/publish.sh standard

# 依次 core → standard（默认）
./scripts/publish.sh

# 指定单个 crate
./scripts/publish.sh tibba-error
```

依赖拓扑图见 [modules.md](modules.md)（执行 `make mermaid` 自动刷新）。
