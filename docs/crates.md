# tibba crate 分层

将 workspace 内可发布模块分为 **核心（Core）**、**标准（Standard）** 与 **扩展（Extension）**，便于：

1. 理解依赖边界与脚手架最小集
2. 分批发布到 crates.io（见 `scripts/publish.sh`）

三层含义：

- **Core** — 最小脚手架底座，几乎所有请求路径都依赖（错误 / 配置 / 缓存 / 出站 HTTP 等）。
- **Standard** — 标准 REST 构件（数据模型、存储、会话、鉴权栈、路由），比 core 低、比 ext 高，依赖核心。
- **Extension** — 可选产品能力，依赖核心与标准。

工具包 `tibba-scaffold` 标记为 **Tool**，`publish = false`，不参与发布。

版本号统一见根 `Cargo.toml` 的 `[workspace.package]`。

## 核心 Core

最小脚手架底座：错误模型、配置、密钥、缓存、出站 HTTP，以及钩子与调度。

| Crate | 职责 |
|-------|------|
| `tibba-error` | HTTP 错误类型 |
| `tibba-state` | AppState / 请求上下文 |
| `tibba-performance` | 进程指标 |
| `tibba-validator` | 校验辅助 |
| `tibba-util` | 通用工具 |
| `tibba-config` | 配置加载 |
| `tibba-crypto` | 密码哈希 / 密钥 |
| `tibba-hook` | 启动/关闭钩子 |
| `tibba-scheduler` | Cron / 重复任务 |
| `tibba-cache` | Redis 缓存 |
| `tibba-request` | 出站 HTTP 客户端 |

## 标准 Standard

标准 REST 构件：数据模型、对象存储、SQL、会话、鉴权栈与通用路由。依赖核心，比 core 低、比 ext 高。

| Crate | 职责 |
|-------|------|
| `tibba-model` | Model 抽象与基础模型 |
| `tibba-opendal` | 对象存储 |
| `tibba-sql` | Postgres 连接池 |
| `tibba-model-builtin` | 内置表模型（文件/用户关联等） |
| `tibba-session` | Cookie + Redis 会话 |
| `tibba-email` | 邮件发送 |
| `tibba-oauth` | OAuth 客户端 |
| `tibba-jwt` | JWT 鉴权 |
| `tibba-totp` | TOTP 两步验证 |
| `tibba-i18n` | 错误消息本地化 |
| `tibba-middleware` | HTTP 中间件栈 |
| `tibba-rbac` | 权限中间件 |
| `tibba-router-common` | 公共路由 |
| `tibba-router-file` | 文件路由 |
| `tibba-router-model` | 通用 Model CRUD 路由 |
| `tibba-router-user` | 用户/鉴权路由 |

## 扩展 Extension

可选产品能力：计费、LLM、任务队列、Webhook、特性开关、多租户等。

| Crate | 职责 |
|-------|------|
| `tibba-job` | PG 异步任务队列 |
| `tibba-llm` | LLM 客户端 |
| `tibba-model-token` | Token 计费模型 |
| `tibba-notify` | 邮件/企微等统一通知 |
| `tibba-webhook` | 出站 Webhook |
| `tibba-feature` | 特性开关 |
| `tibba-tenant` | 行级多租户原语 |

主应用 `tibba` 的 Cargo features 与扩展对应关系（示意）：

| Feature | 相关扩展 |
|---------|----------|
| `demo-token` | `tibba-model-token` |
| `demo-docker` | 应用内模块 + token |
| `demo-detector` | 应用内探测 + 部分 model-builtin |
| `demo-tenant` | `tibba-tenant` |

## 工具 Tool

| Crate | 说明 |
|-------|------|
| `tibba-scaffold` | 生成新项目，不发布 |

## 发布

```bash
# 只发核心（按依赖批次 + 等待 crates.io 索引）
./scripts/publish.sh core

# 只发标准（需核心已在 crates.io）
./scripts/publish.sh standard

# 只发扩展（需核心 + 标准已在 crates.io）
./scripts/publish.sh ext

# 依次 core → standard → ext（默认）
./scripts/publish.sh
./scripts/publish.sh all

# 指定单个 crate
./scripts/publish.sh tibba-error
```

依赖关系图：`docs/modules.md`（`make mermaid` 更新）。
