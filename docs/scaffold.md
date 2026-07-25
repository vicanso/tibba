# 派生新项目

本仓库的主应用（`src/`）**就是**参考实现与模板本身。此前有一个 `tibba-scaffold`
生成器，它维护了一份模板副本（含独立的依赖版本 pin 与一份手工精简的 `main.rs`）；
那份副本无人校验、逐步漂移，最终产出的项目已经无法编译，因此被移除。

> **本文档的编写原则：只做指针，不做副本。**
> 下面不会复述任何依赖版本、代码片段或配置默认值——那些东西都会变。
> 需要它们时请直接读仓库里的对应文件，那里才是唯一真相。

两条路径，按你要什么选：

| 你想要 | 走哪条 | 确定性 |
|--------|--------|--------|
| 一个能立刻跑起来、且可以改框架源码的项目 | [派生仓库](#路径一派生仓库) | 高：产物就是 CI 验证过的代码 |
| 一个只依赖已发布 crate 的独立精简项目 | [自行组装](#路径二自行组装) | 取决于你（或 AI）的执行 |

---

## 路径一：派生仓库

```bash
./scripts/init-project.sh my-app ~/github            # 完整（含全部 demo-* 样板）
./scripts/init-project.sh my-app ~/github --minimal  # default features 置空
./scripts/init-project.sh my-app ~/github --dry-run  # 只看会改什么
```

复制整个仓库（排除 `target/`、`.git/`、`node_modules/`、`admin/dist/`）后改名。
改名范围与不改动 `tibba-*` crate 名的理由见脚本内的 `usage`，或跑 `--help`。

产物保留 workspace 与全部 `tibba-*` **path 依赖**——这既是它不会漂移的原因
（不解析 crates.io 版本），也意味着你可以直接修改框架源码。代价是仓库里带着全部
`tibba-*` crate。

---

## 路径二：自行组装

适合「只想依赖已发布 crate、不想带着框架源码」的场景。下面是需要读的东西和顺序；
可以自己做，也可以把这一节交给 AI 让它照着做。

### 1. 确定要哪些 crate

- 分层与职责：**`docs/crates.md`**（Core / Standard / Extension / 各自的可选 feature）
- crate 间依赖关系图：**`docs/modules.md`**（由 `make mermaid` 生成）

### 2. 确定依赖版本

- 全部第三方依赖版本：根 `Cargo.toml` 的 `[workspace.dependencies]`
- `tibba-*` 版本：根 `Cargo.toml` 的 `[workspace.package].version`
- **不要抄本文档里的版本号（这里一个都没写）**，也不要抄别处的旧模板

新项目的 `Cargo.toml` 需要把 `workspace = true` 展开成具体版本，并去掉
`[workspace]`、`[workspace.package]`、`[workspace.dependencies]` 三个段。

### 3. 复制应用骨架

以 `src/main.rs` 的 `mod` 声明为准（那是唯一权威清单）。当前始终存在的模块：

| 模块 | 职责 |
|------|------|
| `app_ctx` | 应用级共享依赖容器，显式 DI 入口（`AppCtx::install_from_globals`） |
| `config` | 配置加载，`Config::builder().with_env_prefix(...)` |
| `state` | `AppState` 构造 + 启停钩子 + cron 任务注册 |
| `cache` | Redis 客户端与命令统计 |
| `sql` | Postgres 连接池 |
| `dal` | OpenDAL 对象存储 |
| `router` | 路由组装 |
| `admin_web` | rust-embed 挂载管理端 SPA |
| `metrics` | Prometheus 指标接入 |
| `openapi` | OpenAPI 文档与 Swagger UI（仅 dev/test 挂载） |
| `i18n` | 应用级错误消息中英文译文登记 |
| `feature` | 特性开关路由（`/features`，需 Admin） |
| `job` | 应用级异步任务 handler 注册 |

其余模块（`docker`、`model`、`httpstat`、`llm`、`tenant`）全部由 `demo-*` feature
门控，是可删的样板业务——见下一节。

### 4. 决定要不要样板业务

根 `Cargo.toml` 的 `[features]` 段定义了 `demo-docker` / `demo-detector` /
`demo-tenant` / `demo-token`，`src/main.rs` 与 `src/router.rs` 用
`#[cfg(feature = "demo-*")]` 门控对应模块与路由。

所以**「精简版应用」不需要手写**：`cargo build --no-default-features` 就是。
这也正是原 scaffold 那份手工精简 `main.rs` 会漂移的根因——它重复了 feature flag
已经提供的能力。

### 5. 配置、数据库、前端

- 配置项的完整清单与默认值：`configs/default.toml`（唯一真相）
- 环境变量覆盖规则（前缀、`__` 分隔符、大小写）：`tibba-config/README.md`
- **生产必须覆盖**的键（密钥长度要求、CORS 白名单等）：根 `README.md` 的环境变量表
- 数据库 schema：`sql/pg/init.sql`
- 管理端 SPA：`admin/`（`npm install && npm run build`）

### 6. 自检

组装完至少要过这三条：

```bash
cargo clippy --all-targets -- --deny=warnings   # 与本仓库同标准（unwrap_used = deny）
cargo build --no-default-features               # 若你保留了 demo-* feature
cargo run                                        # 起得来，/healthz 返回 ok
```

---

## 给 AI 的提示

把「路径二」整节连同这几个文件一起给它，让它自己读，不要把内容粘贴进 prompt：

```
docs/crates.md      分层与 feature
docs/modules.md     依赖图
Cargo.toml          版本真相（workspace.dependencies / workspace.package）
src/main.rs         模块权威清单 + demo-* 门控
src/router.rs       路由组装方式
configs/default.toml 配置项真相
CLAUDE.md           本项目编码规范（snafu / 链式配置 / LOG_TARGET / 中文注释等）
```

`CLAUDE.md` 这条别漏——新项目若要沿用本仓库的错误处理与链式配置约定，那里是规范出处。
