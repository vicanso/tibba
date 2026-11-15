# tibba-opendal

**对象存储**

> **分层**：标准（Standard）— 标准 REST 构件，依赖核心

基于 OpenDAL 的统一存储抽象，支持通过 Cargo feature 按需开启存储后端：

- `fs`（默认启用）：本地文件系统（`file://`）
- `s3`（可选）：S3 / R2 / MinIO 等兼容对象存储
- `http`（可选）：只读 HTTP 后端（`schema = "http"`）

提供分段安全配置、路径规范化校验与文件流读写，供文件上传下载使用。

## 依赖

依赖：`tibba-config`、`tibba-error`、`tibba-util`

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
