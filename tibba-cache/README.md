# tibba-cache

**Redis 缓存**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

连接池、KV/结构体缓存、分布式锁、计数器（限流/防爆破）与可选压缩。

## 连接模型

| API | 生命周期 | response timeout | 适用 |
|-----|----------|------------------|------|
| `conn()` | 借池 → drop 归还 | 池默认 | GET/SET/INCR |
| `dedicated_blocking_conn(max_block)` | 独立建连 → drop 关闭 | **`max_block+1s` 或无限** | **`BRPOP` / ferry pull_loop** |
| `dedicated_command_conn()` | 独立建连 → drop 关闭 | 显式 **5s** | ferry reply_loop 短写 |

### 为何 `dedicated_blocking_conn` 必须带 `max_block`

redis-rs 1.x 默认 `DEFAULT_RESPONSE_TIMEOUT = 500ms`。只解决「不归池」而不改超时，`BRPOP` 阻塞 2 秒仍会 `error=timed out`。

因此 API 把**服务端最长阻塞时长**做进签名，并自动设置客户端 timeout：

| `max_block` | BRPOP 参数 | 客户端 response_timeout |
|-------------|------------|-------------------------|
| `Duration::ZERO` | `BRPOP key 0` | `None`（关闭） |
| `Duration::from_secs(2)` | `BRPOP key 2` | `3s`（+1s 裕量） |

```rust
use tibba_cache::{brpop_timeout_secs, response_timeout_for_max_block};
use std::time::Duration;

let max_block = Duration::from_secs(2);
assert_eq!(brpop_timeout_secs(max_block), 2);
assert_eq!(
    response_timeout_for_max_block(max_block),
    Some(Duration::from_secs(3))
);

// ferry pull_loop
let mut pull = cache.dedicated_blocking_conn(max_block).await?;
let item: Option<(String, String)> = redis::cmd("BRPOP")
    .arg("ferry:q")
    .arg(brpop_timeout_secs(max_block))
    .query_async(&mut pull)
    .await?;

// ferry reply_loop：另一条专用短写连接
let mut reply = cache.dedicated_command_conn().await?;
```

切勿在 `conn()` 池化连接上跑长阻塞 `BRPOP`。

## URI 参数

```
redis://127.0.0.1:6379?pool_size=20&response_timeout=5s&slow=200ms
```

| 参数 | 默认 | 说明 |
|------|------|------|
| `pool_size` | `10` | 连接池大小 |
| `connection_timeout` | `3s` | 建连超时（池化 + 专用连接同时生效） |
| `wait_timeout` | `3s` | 从池中等待可用连接的超时 |
| `recycle_timeout` | `300ms` | 归还前健康检测（PING）超时 |
| `idle_timeout` | `10m` | 空闲超过即丢弃，不复用 |
| `max_conn_age` | `24h` | 连接最大存活时间 |
| `response_timeout` | `5s` | **单次命令响应超时；`0` = 不超时** |
| `slow` | `200ms` | 慢命令阈值，交由应用侧 `stat_callback` 判定 |

时长均为 humantime 格式（`5s` / `200ms` / `10m` / `24h`）。

### `response_timeout` 为什么必须可配

redis-rs 1.x 默认 **500ms**，受影响的不只是阻塞命令——大 pipeline、慢 Lua 脚本、大范围 `SCAN` 都会被截断。
deadpool-redis 自带 Manager 不暴露该配置，因此单节点与集群都改用自建 Manager，在建连时注入 URI 值，保证**池内**连接也生效。

## 慢命令统计

`stat_callback` 收到 `RedisCmdStat { cmd, elapsed, error, intentional_block }`。

`BRPOP` 正常就要阻塞数秒，无条件计入慢命令会让它长期霸榜、把真实慢查询淹掉。因此
`is_intentional_blocking_command` 识别出的命令（`BRPOP` / `BLPOP` / `BRPOPLPUSH` / `BLMOVE` /
`BLMPOP` / `BZPOPMIN` / `BZPOPMAX` / `BZMPOP` / `SUBSCRIBE` 族 / `WAIT` / 带 `BLOCK` 的
`XREAD`、`XREADGROUP`）在**成功**时直接跳过回调；出错仍上报（连接断开需要被看见），
并带 `intentional_block = true` 供调用方分流。

## 热点路径与扩展原语

- **原子消费（`GETDEL`）**：`RedisCache::get_del` / `get_del_struct` 在单条 Redis 命令内读取并删除键，避免 MFA challenge / OAuth state 等一次性令牌的 TOCTOU 竞态
- **分布式锁与限流**：`lock`（仅靠 TTL 释放，适合定时任务去重）、`lock_with_token` + Lua CAS 释放、`rate_limit_sliding`（滑动窗口跨实例共享限流）、`eval`（EVALSHA 自动回退）、`get_or_set`（进程内 singleflight 回源合并）
- **两级缓存与跨节点失效**：`TwoLevelStore`（进程内 L1 对齐墙钟边界 + `DEFAULT_L2_JITTER_PERCENT` 防雪崩抖动）+ `InvalidationBus` / `InvalidationListener`（Redis pub/sub 广播失效 key、跳过本节点 `node_id`、指数退避自动重连）
- **凭证撤销**：`revocation` 模块提供 `revoke_user_credentials` / `revoke_role_credentials` / `is_credential_revoked`
- **配置脱敏**：`RedisConfig` 的 `Debug` 实现自动屏蔽 `password` 与节点 URI 中的 userinfo（`scheme://***@host:port`）

## 依赖

依赖：tibba-config, tibba-error, tibba-util

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
