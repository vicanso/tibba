# tibba-auth

**认证与授权**

> **分层**：标准（Standard）— 合并原 `tibba-session`、`tibba-rbac`（中间件）、`tibba-jwt`、`tibba-totp`、`tibba-oauth`

提供统一的认证与授权能力（不依赖 `sqlx` 或 `tibba-model`）：

- **Session + Cookie（默认）**：签名 Cookie + Redis 会话存储、`UserSession` / `AdminSession` 提取器、按用户或角色批量撤销会话
- **RBAC（默认）**：`permission_grants` 通配符权限匹配与 `require_permission` 路由中间件
- **`jwt` feature（可选）**：HS256 JWT 无状态访问令牌与 Redis 撤销检查
- **`totp` feature（可选）**：RFC 6238 TOTP 动态码生成/校验、恢复码与密钥加密
- **`oauth` feature（可选）**：GitHub / Google OAuth2 客户端

## 依赖

依赖：`tibba-cache`、`tibba-error`、`tibba-runtime`、`tibba-util`（开启对应 feature 时按需依赖 `tibba-crypto`、`tibba-request`）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
