# tibba-router-model

**通用 Model 路由**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

Schema 驱动的动态模型 CRUD 路由、`DynModel` / `ModelAdapter` 注册表及按模型权限码与调用方上下文（`ModelCaller`）的增删改查校验。

## 依赖

依赖：`tibba-error`、`tibba-model`、`tibba-model-builtin`、`tibba-session`、`tibba-util`

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
