# tibba-util

**通用工具**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

时间、URI、压缩、HTTP 辅助、环境判断等横切小工具，以及 `validator` crate
的自定义校验器。

## 校验器（`validate` 模块）

`x_*` 校验函数与 `CODE_*` 错误码常量，直接从 crate 根导出：

```rust
use tibba_util::{x_user_account, x_uuid, x_file_name};

#[derive(Validate)]
struct LoginParams {
    #[validate(custom(function = "x_user_account"))]
    account: String,
}
```

每个校验器可通过同名环境变量临时关闭（`-` 换 `_`、转小写、值设为 `*`），
便于本地开发绕过格式限制：`x_user_password='*'`。

## Features

默认全开，现有调用方无需改动；只用轻量函数的下游可裁剪：

| Feature | 默认 | 内容 | 甩掉的依赖 |
|---------|------|------|-----------|
| `compression` | 开 | `compress` / `decompress` / `Algorithm` | zstd（含 zstd-sys C 编译）、lz4_flex |
| `http` | 开 | header / cookie / body 辅助（`get_header_value`、`generate_device_id_cookie` 等） | axum-extra、cookie、http-body-util |

```toml
# 只要 parse_uri / timestamp / uuid 等轻量函数
tibba-util = { version = "0.2.6", default-features = false }
# 加压缩
tibba-util = { version = "0.2.6", default-features = false, features = ["compression"] }
```

对应的 `Error` 变体随 feature 一并裁剪（如 `Zstd` 仅在 `compression` 开启时存在）。

## 依赖

依赖：tibba-error

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
