# tibba-jwt

**JWT 鉴权**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

HS256 access token 签发/校验器（`jsonwebtoken` + `aws_lc_rs`）与凭证撤销检查，配合 `tibba-router-user` 的 Redis opaque refresh 轮换实现无状态短期鉴权。

## 依赖

依赖：`tibba-cache`、`tibba-error`、`tibba-util`

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
