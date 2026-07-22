# tibba-scaffold

**项目脚手架**

> **分层**：工具（Tool）— 不发布到 crates.io

从模板生成新 tibba 应用（publish = false，不发布到 crates.io）。

## 依赖

无内部 path 依赖

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
