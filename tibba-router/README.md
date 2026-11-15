# tibba-router

聚合 HTTP 业务路由、动态模型管理、RBAC 管理辅助以及 OpenAPI 文档拼接，是应用装配层调用的统一路由模块。

## 模块划分

- `common`：`/healthz`、`/readyz`、`/commons/application`（默认开启；启用 `process-info` feature 采集 CPU/内存/FD/I/O）。
- `user`：会话登录、注册、个人资料、邮箱验证、密码重置，以及按 feature 裁剪的可选身份能力（`jwt`、`totp`、`oauth`、`api-key`）。
- `model`：基于 schema 注册表的通用后台 CRUD 路由（`/models/*`）。
- `user_admin`：`user` 模型安全适配器，保护角色变更与会话吊销。
- `rbac`：`RbacAdmin`——带格式校验、事务写库与活跃会话批量吊销的 RBAC 管理员操作门面。
- `file`（`file` feature）：multipart 流式上传、安全头覆盖预览、`Range` 分片下载与 S3 预签名接口。
- `job`（`job` feature）：后台任务队列状态与死信重放/清理接口（`/jobs/*`）。
