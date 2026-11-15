# tibba-model

**PostgreSQL 连接池与标准数据模型**

> **分层**：标准（Standard）— 合并原 `tibba-sql`、`tibba-model`、`tibba-model-builtin` 与 `tibba-job`

提供 PostgreSQL 连接池初始化（`sql::new_pg_pool`、`sql::ping_db`）、通用 `Model` trait、分页/排序白名单、Schema 视图、8 个脚手架基础数据模型，以及可选的基于 Postgres `FOR UPDATE SKIP LOCKED` 的异步任务队列（`job` feature → `tibba_model::job`）：

- `UserModel`（用户与认证凭据）
- `ConfigurationModel`（动态配置）
- `FileModel`（上传文件元数据）
- `PermissionModel` / `RolePermissionModel`（RBAC 权限字典与角色授权）
- `UserOauthLinkModel`（第三方 OAuth 绑定）
- `ApiKeyModel`（API Key 凭据）
- `AuditLogModel`（仅追加审计日志）

## Features

| Feature | 默认 | 说明 |
|---------|------|------|
| `job` | 关 | 启用 `tibba_model::job`（Postgres `FOR UPDATE SKIP LOCKED` 异步任务队列、指数退避重试、崩溃回收与死信队列） |

## 依赖

依赖：`tibba-config`、`tibba-error`、`tibba-crypto`、`tibba-util`（启用 `job` feature 时额外引入 `dashmap`、`metrics`、`tokio/time`、`tokio/sync`）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
