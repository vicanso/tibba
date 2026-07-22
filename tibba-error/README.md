# tibba-error

**HTTP 错误类型**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

统一 HTTP 错误类型，支持 category / sub_category / status / 链式配置，可直接 `IntoResponse`。

## 依赖

无内部依赖

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
