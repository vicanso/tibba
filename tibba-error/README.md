# tibba-error

**HTTP 错误类型**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

统一 HTTP 错误类型 `Error`，支持 category / sub_category / code / status / extra 链式配置，可直接 `IntoResponse`。

## 核心特性

- **紧凑内存布局**：可选附加字段按需装箱为 `ErrorData`，常规错误零额外堆分配
- **告警与脱敏解耦**：`with_exception(bool)` 控制是否告警，`with_redact(bool)` 独立控制客户端响应是否隐藏 `message` / `extra`（默认 5xx 隐去、4xx 保留），支持 `client_payload`
- **上游错误重建**：`Error::from_upstream(status, body)` 自动解析上游 tibba 结构化错误或按字符边界截断纯文本响应（上限 `MAX_UPSTREAM_MESSAGE_LEN` = 512 字节），并强制按本方状态码重判 `exception`

## 依赖

无内部依赖

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
