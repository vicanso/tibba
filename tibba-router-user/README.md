# tibba-router-user

**用户路由**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

注册登录、Session/JWT/API Key、OAuth、TOTP、邮箱验证、密码重置等用户域 API。

## 依赖

依赖：cache/crypto/email/error/jwt/middleware/model/model-builtin/oauth/session/totp/util/validator

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
