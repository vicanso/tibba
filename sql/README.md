# SQL 脚本说明

## 真相来源：`migrations/`

运行时通过 `sqlx::migrate!("./migrations")` **仅应用**仓库根目录
[`migrations/`](../migrations/) 下的迁移。版本追踪表为 `_sqlx_migrations`。

本地 / CI / 生产请只维护 `migrations/`，不要在本目录新增「应上线」的 schema。

## 本目录（`sql/pg/`）的定位

| 路径 | 用途 |
|------|------|
| `sql/pg/*.sql` | **参考 / 历史** 全量建表脚本（早期手工初始化） |
| `sql/pg/init.sql` | 可能用于本地一次性初始化文档示例 |

与 `migrations/` **可能不同步**。若两边冲突，以 `migrations/` 为准。

> 历史上有 16 张表（`users`、`configurations`、`files`、`token_*` 等）只存在于本目录，
> 从未进入 `migrations/`，导致按 README 从空库启动时迁移失败。现由
> `migrations/20260101000000_baseline_legacy_tables.sql` 补齐：全新库上建表，
> 既有库上整块跳过（每张表包在「不存在才建」的 `DO` 块里）。

## 贡献约定

1. Schema 变更 → 新增 `migrations/YYYYMMDDHHMMSS_description.sql`
2. 不要修改已合并的 migration 文件
3. 若需保留可读的「全表快照」，可在 PR 中更新 `sql/pg/`，并注明「非运行时路径」
