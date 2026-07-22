# tibba-config

**配置加载**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

多文件 TOML + 环境变量前缀覆盖的配置加载与子配置切片。

## 用法

```rust
let config = Config::builder()
    .add_toml(default_toml)          // 打底
    .add_toml(env_toml)              // 覆盖前者
    .with_env_prefix("TIBBA_WEB")    // 优先级最高
    .build()?;

let db = config.sub_config("database");
let uri = db.get_string("uri")?;
let timeout = db.get_duration("timeout")?;              // "30s" 或纯数字秒
let max_body = config.get_byte_size("basic.max_body")?; // "10MB"
```

## 环境变量覆盖

层级分隔符是 **`__`**（双下划线），单 `_` 保留为字段名的一部分：

| 环境变量 | 配置键 |
|----------|--------|
| `TIBBA_WEB__DATABASE__URI` | `database.uri` |
| `TIBBA_WEB__EMAIL__API_KEY` | `email.api_key` |

规则：

- **不调用 `with_env_prefix` 就完全不挂载环境变量源**。空前缀不会被透传给 config-rs —— 那会让 `prefix_pattern` 退化成 `"__"`，等于要求所有变量以 `__` 开头，覆盖能力静默失效。
- 空值视为未设置（`ignore_empty`）：`export TIBBA_WEB__BASIC__SECRET=` 不会把 TOML 里的值抹成空串。
- 分隔符可用 `with_env_separator` 覆盖。

## 依赖

依赖：tibba-error

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
