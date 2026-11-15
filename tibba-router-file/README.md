# tibba-router-file

**文件路由**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

文件流式上传（单文件 50 MiB / 单次总计 200 MiB / 最多 20 文件限制）、图片安全处理、下载/预签名及元数据管理 API，对接 OpenDAL 与 `File` 模型。

## 依赖

依赖：`tibba-error`、`tibba-model-builtin`、`tibba-opendal`、`tibba-session`、`tibba-util`

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
