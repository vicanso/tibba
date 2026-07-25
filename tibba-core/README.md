# tibba-core

**核心组件门面（facade）**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

**不含任何业务代码**，只把 7 个 core crate 收拢到一个依赖项下，让下游从写 7 行依赖、
对齐 7 个版本号变成写一行。

```toml
tibba-core = "0.2.6"
```

```rust
use tibba_core::error::Error;
use tibba_core::runtime::AppState;
use tibba_core::cache::RedisCache;
```

`tibba_core::error` **就是** `tibba_error`（纯 re-export），所以类型完全同一——
同时直接依赖 `tibba-error` 也不会产生两套 `Error`。

## 模块与 feature

| 模块 | 对应 crate | feature | 独占的重依赖 |
|------|-----------|---------|-------------|
| `error` | `tibba-error` | 始终编译 | axum |
| `util` | `tibba-util` | `util` | chrono / uuid / zstd / validator |
| `config` | `tibba-config` | `config` | config-rs |
| `crypto` | `tibba-crypto` | `crypto` | argon2 / hmac / sha2 |
| `runtime` | `tibba-runtime` | `runtime` | arc-swap / dashmap |
| `cache` | `tibba-cache` | `cache` | redis / deadpool |
| `request` | `tibba-request` | `request` | reqwest / opentelemetry |

另有两个透传 feature：`process-info`（`runtime` 的 sysinfo 进程采样）、
`scheduler`（`runtime` 的 cron 定时任务）。

`default = ["full"]` 全部打开——门面的价值就是「写一行就能用」，若默认精简则调用方
又得回去逐个列 feature，等于没解决问题。需要瘦身时：

```toml
# 只要错误类型：55 个传递依赖（full 是 269 个，其中含 process-info 的 sysinfo
# 与 scheduler 的 tokio-cron-scheduler 两族约 28 个）
tibba-core = { version = "0.2.6", default-features = false }

# 按需组合
tibba-core = { version = "0.2.6", default-features = false, features = [
    "config",
    "cache",
] }
```

## 什么时候**不**该用本 crate

**库 crate 应当继续直接依赖它真正需要的那几个 core crate**，不要经由门面。
7 个 crate 的边界本身有价值：

- **依赖可裁剪** —— `tibba-sql` 只用到 `tibba_util::parse_uri`，不该背上 redis 与 reqwest。
- **增量编译粒度** —— 改 `tibba-cache` 一行只重编 cache 及其下游，不会波及全部 core
  （合成一个 crate 就是 6538 行的单一编译单元）。
- **分层由编译器强制** —— 跨层引用直接编译失败，而不是靠自觉。

门面是给**最终应用**用的：那里本来就要用到全部 core，收拢依赖是净收益。
workspace 内的 `tibba-*` 均**不**依赖本 crate，正是这个原因。

## 依赖

依赖：tibba-error（始终）、tibba-util / tibba-config / tibba-crypto /
tibba-runtime / tibba-cache / tibba-request（各自 feature 门控）

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：`scripts/publish.sh` 的 `core/C4` 批次，**必须在其余 7 个之后**发布

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
