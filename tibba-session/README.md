# tibba-session

**会话**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

签名 Cookie + Redis Session 会话管理：支持登录会话 ID 轮换、服务端销毁并下发清除 Cookie、`UserSession`/`AdminSession`/`SessionResponse` 提取器、`CTX` 账号同步及细粒度通配符权限判定。

## 依赖

依赖：`tibba-cache`、`tibba-error`、`tibba-runtime`、`tibba-util`（测试依赖：`tibba-config`）

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
