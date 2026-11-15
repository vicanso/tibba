# tibba

基于 **Axum + PostgreSQL + Redis** 的轻量级 Rust Web 服务脚手架。

Workspace 内聚为 **12 个 `tibba-*` crate**（7 个 Core + 5 个 Standard）；依赖关系图见 [modules.md](docs/modules.md)。可选能力全部通过根 `Cargo.toml` feature flag 按需裁剪。

## 架构速览

```text
HTTP 中间件栈（tibba_middleware::apply_middleware_stack）
  request_id → [otel_trace] → security_headers → cors → compression
  → [i18n] → timeout → body_limit → entry → stats → processing_limit
  → session → [api_key_auth] → [validate_csrf] → [http_cache]
       ↓
  Router（src/router.rs → tibba-router）
  /healthz · /readyz · /commons/* · /users/* · /models/* · [/files/*] · [/jobs/*] · /metrics · /csrf/token
       ↓
  AppCtx（src/app_ctx.rs::init 显式初始化：state · pool · cache · [storage]）
       ↓
  sqlx（PgPool + ./migrations）· RedisCache · OpenDAL · JobQueue
```

## 快速开始

### 依赖

- Rust toolchain（`rust-version` 见 [Cargo.toml](Cargo.toml)）
- PostgreSQL 14+
- Redis 6+
- （可选）Node 20+：仅构建 `admin/` SPA（`embed-admin` feature）时需要

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

> Schema 在应用启动时由 `sqlx::migrate!("./migrations")` 自动执行（见 [migrations/](migrations/)）。

### 2. Redis

```bash
docker run -d --name tibba-redis -p 6379:6379 redis:7-alpine
```

### 3. 配置

编译期嵌入 `configs/default.toml` + `configs/{ENV}.toml`（见 [configs/](configs/)），可用环境变量覆盖，前缀 **`TIBBA_WEB__`**（密钥字段均支持同名 `*_FILE` 后缀从挂载文件读取，如 `TIBBA_WEB__BASIC__SECRET_FILE`）：

| 变量 | 说明 |
|------|------|
| `TIBBA_WEB__BASIC__SECRET` | 生产**必须**覆盖（≥32 字符） |
| `TIBBA_WEB__BASIC__PASSWORD_PEPPER` | 可选密码哈希服务端 pepper（经 `tibba-crypto` 全局策略生效） |
| `TIBBA_WEB__BASIC__CORS_ALLOW_ORIGINS` | 生产**必须**配置来源白名单 |
| `TIBBA_WEB__BASIC__SUPER_ADMINS` | 启动时授予 `su` 超管的账号列表（幂等） |
| `TIBBA_WEB__BASIC__LISTEN` | 监听地址，默认 `127.0.0.1:5000` |
| `TIBBA_WEB__DATABASE__URI` | PostgreSQL 连接串 |
| `TIBBA_WEB__REDIS__URI` | Redis 连接串 |
| `TIBBA_THREADS` | Tokio worker 线程数（默认 CPU 核数） |
| `RUST_LOG` | 日志级别，如 `info,tibba:cache=debug` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | 可选 OTLP 追踪端点（需开启 `otel` feature） |

开发配置见 [dev.toml](configs/dev.toml)。

### 4. 运行与检查

```bash
# 未设置 RUST_ENV 时按 production 运行（拒绝开发占位 secret、要求 CORS 白名单、不挂 OpenAPI UI）
RUST_ENV=dev cargo run

# 或（bacon.toml 已配置 RUST_ENV=dev）
make dev

# 质量检查
make fmt
make lint
cargo test --workspace
```

服务启动后主要端点（默认 `basic.prefix = "/api"`）：

- 存活探针：`GET /api/healthz`
- 就绪探针：`GET /api/readyz`（探测 DB / Redis / 可选 OpenDAL / 可选任务积压）
- 接口文档：dev/test 下 `GET /swagger-ui`（Scalar API Reference）与 `GET /api-docs/openapi.json`
- 指标输出：`GET /api/metrics`

### 5. Admin 前端

```bash
cd admin && npm ci && npm run dev
```

从后端导出全量 OpenAPI 并生成 TypeScript 类型：

```bash
make openapi          # → admin/openapi.json（使用 --features full）
make openapi-types    # → admin/src/api/schema.d.ts
```

## 派生新项目

本仓库主应用（`src/`）即参考实现与脚手架模板，使用说明见 [scaffold.md](docs/scaffold.md)：

```bash
make init name=my-app dest=~/github                  # 复制仓库并改名
make init name=my-app dest=~/github flags=--minimal  # 将 default features 置空
```

## Cargo Features 与精简构建

根包默认开启 `default = ["file", "job", "email"]`；其余能力按需打开，全量启用使用 `full`：

| Feature | 说明 |
|---------|------|
| `file`（默认开） | 文件上传 / 下载 / 预览与对象存储抽象（本地 fs） |
| `s3` | S3 兼容对象存储后端（隐含 `file`） |
| `job`（默认开） | PostgreSQL 异步任务队列与 `/jobs/*` 运维路由 |
| `email`（默认开） | 邮件发送、邮箱验证与密码重置能力 |
| `jwt` | JWT 登录 / 刷新 / 登出与无状态 access 校验 |
| `totp` | TOTP 两步验证 |
| `oauth` | GitHub / Google OAuth2 登录 |
| `api-key` | API Key（PAT）管理与鉴权中间件 |
| `otel` | OpenTelemetry 分布式追踪（入站 / 出站 / OTLP 导出） |
| `process-info` | 进程 CPU / 内存 / FD / 磁盘 I/O 采样 |
| `scheduler` | 进程内 cron 周期监控任务调度器 |
| `embed-admin` | 编译期嵌入 `admin/dist/` SPA 静态资源 |
| `full` | 启用上述全部 12 个可选 feature |

```bash
# 最小二进制（关闭 file、job 与 email）
make release-minimal

# 内嵌 admin SPA 构建
make release-embed

# 按需组合
cargo build --release --no-default-features --features "jwt,totp,api-key"
```

## 分层与发布

Workspace 分为 **Core（7 crates）** 与 **Standard（5 crates）**，详细说明见 [crates.md](docs/crates.md)：

```bash
./scripts/publish.sh core      # 仅核心层
./scripts/publish.sh standard  # 仅标准层（需 core 已发布）
./scripts/publish.sh           # core → standard 依次发布
```

## Docker

```bash
docker build --build-arg GIT_COMMIT_ID=$(git rev-parse HEAD) -t tibba .
```

多阶段构建：Node 构建 `admin/` SPA → Rust (`make release-embed`) → `debian:bookworm-slim` 运行时。

## License

Apache-2.0
