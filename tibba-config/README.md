# tibba-config

**配置加载**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

多文件 TOML + 环境变量前缀覆盖的配置加载与子配置切片。

## 依赖

依赖：tibba-error

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
