# tibba

基于 **Axum + PostgreSQL + Redis** 的 Rust REST 服务脚手架 / 全功能应用底座。

提供会话与多种鉴权（Cookie Session / JWT / API Key）、RBAC、限流、CSRF、任务队列、对象存储、OpenAPI、Admin SPA 等横切能力，可按需裁剪后用于新业务。

## 架构速览

```
HTTP 中间件栈
  request_id → otel → security headers → CORS → ETag → i18n
  → entry/stats → session → api_key → csrf → processing_limit
       ↓
  Router（users / files / models / jobs / features / tenant / …）
       ↓
  AppCtx（pool · cache · storage · AppState）
       ↓
  sqlx · Redis · OpenDAL · JobQueue
```

Workspace 内约 35 个 `tibba-*` crate；依赖关系图见 [docs/modules.md](docs/modules.md)。

## 快速开始

### 依赖

- Rust **1.85+**（edition 2024）
- PostgreSQL 14+
- Redis 6+
- （可选）Node 20+：构建 `admin/` SPA

### 1. 数据库

```bash
docker run -d --restart=always \
  -v $PWD/postgres:/var/lib/postgresql \
  -e POSTGRES_PASSWORD=A123456 \
  -p 5432:5432 \
  --name=tibba-postgres \
  postgres:18-alpine

docker exec -it tibba-postgres psql -U postgres -c "CREATE USER vicanso WITH PASSWORD 'A123456';"
docker exec -it tibba-postgres psql -U postgres -c "CREATE DATABASE cybertect OWNER vicanso;"
```

> Schema 由运行时 `sqlx::migrate!` 自动应用（见 `migrations/`）。`sql/pg/` 仅为历史参考，见 [sql/README.md](sql/README.md)。

### 2. Redis

```bash
docker run -d --name tibba-redis -p 6379:6379 redis:7-alpine
```

### 3. 配置

默认读 `configs/default.toml` + `configs/{ENV}.toml`，可用环境变量覆盖，前缀 **`TIBBA_WEB__`**：

| 变量 | 说明 |
|------|------|
| `TIBBA_WEB__BASIC__SECRET` | 生产**必须**覆盖（≥32 字符） |
| `TIBBA_WEB__BASIC__CORS_ALLOW_ORIGINS` | 生产**必须**配置来源白名单 |
| `TIBBA_WEB__BASIC__LISTEN` | 监听地址，默认 `127.0.0.1:5000` |
| `TIBBA_WEB__DATABASE__URI` | Postgres 连接串 |
| `TIBBA_WEB__REDIS__URI` | Redis 连接串 |
| `RUST_LOG` | 日志级别，如 `tibba:app=info,tibba:cache=debug` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | 可选 OTLP 追踪 |
| `TIBBA_PANIC_WECOM_KEY` | 可选 panic 企业微信告警 |

开发配置见 `configs/dev.toml`。

### 4. 运行

```bash
# 开发（需 bacon 可选）
cargo run

# 或
make dev   # bacon run

# 检查
make fmt
make lint
cargo test --workspace
```

服务起来后：

- 健康：`GET /api/ping`（若配置了 `basic.prefix`）
- 就绪：`GET /api/readyz`（DB / Redis / 存储 / 任务积压）
- 文档：dev/test 下 `GET /swagger-ui`
- 指标：`GET /api/metrics`

### 5. Admin 前端

```bash
cd admin && npm ci && npm run dev
```

从后端导出 OpenAPI 并生成 TS 类型：

```bash
make openapi          # → admin/openapi.json
make openapi-types    # openapi-typescript → admin/src/api/schema.d.ts
```

## 脚手架新项目

```bash
make scaffold name=my-app
# 或
cargo run -p tibba-scaffold -- my-app ~/github
```

模板会生成最小可运行入口（AppCtx + 核心中间件）；可按业务再引入 `tibba-router-*` 等 crate。

## 常用能力入口

| 能力 | 位置 |
|------|------|
| 用户 / 登录 / OAuth / TOTP / JWT | `tibba-router-user` |
| 通用 Model CRUD + 权限码 | `tibba-router-model` |
| 文件上传 | `tibba-router-file` + OpenDAL |
| 异步任务队列 | `tibba-job` + `src/job.rs` |
| 行级多租户 | `tibba-tenant` + `src/tenant.rs` |
| 中间件开关 | `MiddlewareOptions`（`tibba-middleware`） |
| 共享依赖 | `AppCtx`（`src/app_ctx.rs`） |

## Cargo features（可选样板）

默认 `full` 与历史行为一致。构建更瘦的二进制可关掉样板业务：

| Feature | 内容 |
|---------|------|
| `demo-docker`（默认 on） | `/docker/analyze`、docker 分析任务 |
| `demo-detector`（默认 on） | HTTP 探测调度（`http-stat` + `quick-js`） |
| `demo-tenant`（默认 on） | `/tenant` 多租户演示 |
| `full` | 上述全部 |

```bash
# 最小：仅用户/文件/model/job/features 等核心
cargo build --release --no-default-features
# 或 make release-minimal

# 只要租户演示
cargo build --no-default-features --features demo-tenant
```

## Docker

```bash
docker build --build-arg GIT_COMMIT_ID=$(git rev-parse HEAD) -t tibba .
```

多阶段：Node 构建 admin → Rust release → **debian:bookworm-slim** 运行时。

## License

Apache-2.0
