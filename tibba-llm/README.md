# tibba-llm

**LLM 客户端**

> **分层**：扩展（Extension）— 可选能力，依赖核心后再发布

OpenAI 兼容与 Anthropic 协议的 LLM HTTP 客户端封装。

## 依赖

依赖：tibba-error

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
