# 派生新项目

本仓库的主应用（`src/`）即参考实现与脚手架模板。

> **编写原则：只做指针，不做副本。**
> 本文不复制依赖版本、源码片段或配置默认值，所有内容均直接指向仓库内对应文件。

两种起步方式：

| 场景 | 方式 | 特点 |
|------|------|------|
| 希望保留完整 workspace 并能修改 `tibba-*` 源码 | [路径一：派生仓库](#路径一派生仓库) | 零版本漂移，可直接运行与二次定制 |
| 只依赖已发布 crate 的独立服务 | [路径二：自行组装](#路径二自行组装) | 仓库轻量，按需挑选 Core / Standard 模块 |

---

## 路径一：派生仓库

```bash
./scripts/init-project.sh my-app ~/github            # 保留 default features（file + job + email）
./scripts/init-project.sh my-app ~/github --minimal  # 将 default features 置为 []
./scripts/init-project.sh my-app ~/github --dry-run  # 预览替换内容
```

脚本会复制整个仓库（排除 `target/`、`.git/`、`node_modules/`、`admin/dist/`），替换主应用 crate 名、`AppState` 服务名、tracing target 前缀、`*_WEB__` 配置前缀以及 `Dockerfile` / `entrypoint.sh` 二进制名，同时保持 `tibba-*` crate path 依赖不变。

---

## 路径二：自行组装

### 1. 选择需要的 crate 与 feature

- 分层职责与 feature 矩阵：[crates.md](crates.md)
- Crate 间依赖拓扑：[modules.md](modules.md)（通过 `make mermaid` 生成）

### 2. 对齐依赖版本

- 第三方 crate 版本与 `tibba-*` 版本号均以根 [Cargo.toml](../Cargo.toml) 为唯一真相。
- 独立项目中展开 `workspace = true` 并去掉 `[workspace]` 相关段落。

### 3. 主应用模块骨架

以 [src/main.rs](../src/main.rs) 为权威入口，当前应用层仅包含 7 个模块：

| 模块 | 文件 | 职责 |
|------|------|------|
| `config` | [config.rs](../src/config.rs) | 同步加载内嵌 TOML 与 `*_WEB__` 环境变量，初始化密码 pepper / JWT / Session / OAuth 配置 |
| `metrics` | [metrics.rs](../src/metrics.rs) | 安装 Prometheus recorder 并提供 `/metrics` 端点 |
| `i18n` | [i18n.rs](../src/i18n.rs) | 注册错误响应中英文译文目录 |
| `app_ctx` | [app_ctx.rs](../src/app_ctx.rs) | 显式初始化 `AppCtx`（`AppState`、`PgPool` + 自动迁移、`RedisCache`、可选 `Storage` 与监控任务） |
| `openapi` | [openapi.rs](../src/openapi.rs) | 聚合 `tibba_router::openapi()` 并在 dev/test 下挂载 Scalar API Reference UI |
| `admin_web` | [admin_web.rs](../src/admin_web.rs) | 启用 `embed-admin` 时以 `rust-embed` 服务 `admin/dist/` SPA |
| `router` | [router.rs](../src/router.rs) | 注册业务模型（`user` / `configuration` / `file`）、构建 readiness 探针并挂载路由树 |

中间件栈统一通过 `tibba_middleware::apply_middleware_stack` 挂载，无需手写冗长的 `.layer(...)` 链。

### 4. 配置、数据库与前端

- 默认配置项：[configs/default.toml](../configs/default.toml)
- 环境变量覆盖与 `*_FILE` 密钥文件机制：[tibba-config README](../tibba-config/README.md)
- 数据库初始 Schema：[migrations/20260101000000_init.sql](../migrations/20260101000000_init.sql)（启动时由 `sqlx::migrate!` 自动应用）
- Admin SPA：[admin/](../admin/)（仅启用 `embed-admin` 时需要 `npm run build` 产出 `admin/dist/`）

### 5. 自检清单

```bash
cargo clippy --all-targets --no-default-features -- --deny=warnings
cargo clippy --all-targets --all-features -- --deny=warnings
cargo test --workspace --all-features
RUST_ENV=dev cargo run
```
