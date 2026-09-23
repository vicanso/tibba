// Copyright 2026 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::{Error, JsonSnafu, SchemaOption, SchemaView, parse_primitive_datetime};
use serde::{Deserialize, Serialize};
use serde_json::json;
use snafu::ResultExt;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use std::future::Future;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ModelListParams {
    pub page: u64,
    pub limit: u64,
    pub order_by: Option<String>,
    pub keyword: Option<String>,
    pub filters: Option<String>,
}

/// 默认允许排序的列：各 model 未覆写 [`Model::orderable_columns`] 时使用。
/// 仅含通用时间戳与主键，避免客户端对敏感列（如 `password`）做 ORDER BY 探测。
pub const DEFAULT_ORDERABLE_COLUMNS: &[&str] = &["id", "created", "modified"];

impl ModelListParams {
    pub fn parse_filters(&self) -> Result<Option<HashMap<String, String>>> {
        if let Some(filters) = &self.filters {
            let filters: HashMap<String, String> =
                serde_json::from_str(filters).context(JsonSnafu)?;
            Ok(Some(filters))
        } else {
            Ok(None)
        }
    }

    /// 追加 `ORDER BY` + `LIMIT/OFFSET`。
    ///
    /// `allowed` 为列名白名单（由 [`Model::orderable_columns`] 提供）；不在名单内的
    /// `order_by` 回退到 `id`（或名单首项），防止任意列排序。
    pub fn push_pagination(&self, qb: &mut QueryBuilder<Postgres>, allowed: &[&str]) {
        let order_by = self.order_by.as_deref().unwrap_or("id");
        push_order_by(qb, order_by, allowed);
        // clamp 到 [1, 200]：加下限 1，避免 limit 缺省 / 传 0 时生成 `LIMIT 0` 静默返回空结果
        let limit = self.limit.clamp(1, 200);
        qb.push(format!(
            " LIMIT {limit} OFFSET {}",
            page_offset(self.page, limit)
        ));
    }
}

/// 由页码与每页条数计算 OFFSET，**饱和**而不溢出。
///
/// `page` 直接来自查询串。此前是 `(page - 1) * limit` 的普通 u64 乘法：
/// `page=18446744073709551615` 在 debug 构建下直接 panic，release 构建（默认
/// 不开 overflow-checks）下静默回绕成一个随机的巨大值，拼进 SQL 后 Postgres 报
/// 「OFFSET 超出 bigint 范围」→ 500。这里饱和到 `i64::MAX`（Postgres OFFSET 的
/// 上限），超出表大小的偏移只会得到空结果。
fn page_offset(page: u64, limit: u64) -> u64 {
    page.max(1)
        .saturating_sub(1)
        .saturating_mul(limit)
        .min(i64::MAX as u64)
}

/// 把用户输入转义成 LIKE 模式里的**字面量**：`\`、`%`、`_` 前加 `\`。
///
/// 此前 `format!("%{keyword}%")` 把关键词原样放进模式，于是：
/// - `keyword=%` 匹配全表，`keyword=_` 匹配任意单字符——搜索框成了「列出一切」；
/// - `%a%b%c%d%…` 这种多通配符模式在大表上是一次代价极高的回溯匹配，一个
///   无需任何权限的列表接口就能拖慢数据库。
///
/// Postgres 的 LIKE 默认转义符就是 `\`，无需额外写 `ESCAPE` 子句。
#[must_use]
pub fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 解析 `order_by`：前缀 `-` 表示 DESC，否则 ASC。
///
/// 返回 `(列名, 方向)`；列名尚未做白名单校验。
#[must_use]
pub fn parse_order_by(order_by: &str) -> (&str, &'static str) {
    if let Some(col) = order_by.strip_prefix('-') {
        (col, "DESC")
    } else {
        (order_by, "ASC")
    }
}

/// 在白名单内时追加 `ORDER BY col DIR`；否则回退到 `id`（或 `allowed` 首项）ASC。
///
/// 额外要求列名仅含字母数字下划线，双保险防止注入。
pub fn push_order_by(qb: &mut QueryBuilder<Postgres>, order_by: &str, allowed: &[&str]) {
    let (col, dir) = parse_order_by(order_by);
    let fallback = if allowed.contains(&"id") {
        "id"
    } else {
        allowed.first().copied().unwrap_or("id")
    };
    let col = if col.chars().all(|c| c.is_alphanumeric() || c == '_') && allowed.contains(&col) {
        col
    } else {
        // 非法 / 未授权列：固定 ASC，避免用攻击者指定的方向排序敏感列
        qb.push(format!(" ORDER BY {fallback} ASC"));
        return;
    };
    qb.push(format!(" ORDER BY {col} {dir}"));
}

/// 要求写操作至少影响一行，否则返回 [`Error::NotFound`]（→ 404）。
///
/// 按 ID 的 `UPDATE` / 软删除 `UPDATE` 在目标不存在（或已被软删）时影响 0 行，
/// 这在 SQL 层面不是错误。此前各 model 一律 `execute(..)?; Ok(())`，于是对一个
/// 不存在的 ID 做更新 / 删除，通用 model 路由回的是成功——调用方以为改到了，
/// 实际什么都没发生。所有 `update_by_id` / `delete_by_id` 都应以它收尾。
pub fn ensure_affected(result: &sqlx::postgres::PgQueryResult) -> Result<()> {
    if result.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

/// 按主键查询未删除行的 `WHERE` 片段（绑定 `$1` = id）。
///
/// sqlx 0.9 的 `query` 仅接受 `'static` 字面量（`SqlSafeStr`），故完整
/// `UPDATE/SELECT` 仍由各 model 写死字面量；此处导出共用片段供 `QueryBuilder::push`。
pub const ACTIVE_BY_ID_WHERE: &str = " WHERE id = $1 AND deleted_at IS NULL";

/// 列表查询的软删除过滤前缀（供 `QueryBuilder::push`）。
pub const ACTIVE_WHERE: &str = " WHERE deleted_at IS NULL";

/// 软删除 `SET` 子句（不含表名），供文档与手工拼接时保持语义一致。
pub const SOFT_DELETE_SET: &str = " SET deleted_at = NOW(), modified = NOW()";

pub trait Model: Send + Sync {
    type Output: Serialize + Send;
    fn new() -> Self;
    fn schema_view<'a>(
        &'a self,
        pool: &'a Pool<Postgres>,
    ) -> impl Future<Output = SchemaView> + Send + 'a;
    /// 允许客户端 `order_by` 使用的列名（不含 `-` 前缀）。
    ///
    /// 默认 [`DEFAULT_ORDERABLE_COLUMNS`]；含敏感字段的表应覆写为明确白名单。
    fn orderable_columns(&self) -> &'static [&'static str] {
        DEFAULT_ORDERABLE_COLUMNS
    }
    fn keyword(&self) -> String {
        String::new()
    }
    fn push_filter_conditions(
        &self,
        _qb: &mut QueryBuilder<Postgres>,
        _filters: &HashMap<String, String>,
    ) -> Result<()> {
        Ok(())
    }
    fn push_conditions(
        &self,
        qb: &mut QueryBuilder<Postgres>,
        params: &ModelListParams,
    ) -> Result<()> {
        qb.push(" WHERE deleted_at IS NULL");

        let col = self.keyword();
        if !col.is_empty()
            && col.chars().all(|c| c.is_alphanumeric() || c == '_')
            && let Some(keyword) = &params.keyword
        {
            qb.push(format!(" AND {col} LIKE "));
            qb.push_bind(format!("%{}%", escape_like(keyword)));
        }

        if let Some(filters) = params.parse_filters()? {
            if let Some(modified) = filters.get("modified")
                && let Some((start, end)) = modified.split_once(',')
            {
                // 与其它时间过滤统一走 parse_primitive_datetime（RFC3339 先归一到 UTC），
                // 绑定 time::PrimitiveDateTime，不再依赖 sqlx 的 chrono feature；
                // 解析失败沿用旧行为：忽略该边界而非报错
                if let Ok(dt) = parse_primitive_datetime(start) {
                    qb.push(" AND modified >= ");
                    qb.push_bind(dt);
                }
                if let Ok(dt) = parse_primitive_datetime(end) {
                    qb.push(" AND modified <= ");
                    qb.push_bind(dt);
                }
            }
            self.push_filter_conditions(qb, &filters)?;
        }

        Ok(())
    }
    fn insert<'a>(
        &'a self,
        _pool: &'a Pool<Postgres>,
        _params: serde_json::Value,
    ) -> impl Future<Output = Result<u64>> + Send + 'a {
        async {
            Err(Error::NotSupported {
                name: "insert".to_string(),
            })
        }
    }
    fn get_by_id<'a>(
        &'a self,
        _pool: &'a Pool<Postgres>,
        _id: u64,
    ) -> impl Future<Output = Result<Option<Self::Output>>> + Send + 'a {
        async {
            Err(Error::NotSupported {
                name: "get_by_id".to_string(),
            })
        }
    }
    fn delete_by_id<'a>(
        &'a self,
        _pool: &'a Pool<Postgres>,
        _id: u64,
    ) -> impl Future<Output = Result<()>> + Send + 'a {
        async {
            Err(Error::NotSupported {
                name: "delete_by_id".to_string(),
            })
        }
    }
    fn update_by_id<'a>(
        &'a self,
        _pool: &'a Pool<Postgres>,
        _id: u64,
        _params: serde_json::Value,
    ) -> impl Future<Output = Result<()>> + Send + 'a {
        async {
            Err(Error::NotSupported {
                name: "update_by_id".to_string(),
            })
        }
    }
    fn count<'a>(
        &'a self,
        _pool: &'a Pool<Postgres>,
        _params: &'a ModelListParams,
    ) -> impl Future<Output = Result<i64>> + Send + 'a {
        async {
            Err(Error::NotSupported {
                name: "count".to_string(),
            })
        }
    }
    fn list<'a>(
        &'a self,
        _pool: &'a Pool<Postgres>,
        _params: &'a ModelListParams,
    ) -> impl Future<Output = Result<Vec<Self::Output>>> + Send + 'a {
        async {
            Err(Error::NotSupported {
                name: "list".to_string(),
            })
        }
    }
    fn list_and_count<'a>(
        &'a self,
        pool: &'a Pool<Postgres>,
        count: bool,
        params: &'a ModelListParams,
    ) -> impl Future<Output = Result<serde_json::Value>> + Send + 'a {
        async move {
            if count {
                let (n, items) =
                    tokio::try_join!(self.count(pool, params), self.list(pool, params))?;
                Ok(json!({ "count": n, "items": items }))
            } else {
                let items = self.list(pool, params).await?;
                Ok(json!({ "count": -1_i64, "items": items }))
            }
        }
    }
    fn search_options<'a>(
        &'a self,
        _pool: &'a Pool<Postgres>,
        _keyword: Option<String>,
    ) -> impl Future<Output = Result<Vec<SchemaOption>>> + Send + 'a {
        async { Ok(vec![]) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn order_sql(order_by: &str, allowed: &[&str]) -> String {
        let mut qb = QueryBuilder::<Postgres>::new("SELECT 1");
        push_order_by(&mut qb, order_by, allowed);
        qb.sql().as_str().to_string()
    }

    #[test]
    fn parse_order_by_asc_and_desc() {
        assert_eq!(parse_order_by("created"), ("created", "ASC"));
        assert_eq!(parse_order_by("-created"), ("created", "DESC"));
    }

    #[test]
    fn push_order_by_allows_whitelisted_column() {
        assert_eq!(
            order_sql("-modified", DEFAULT_ORDERABLE_COLUMNS),
            "SELECT 1 ORDER BY modified DESC"
        );
        assert_eq!(
            order_sql("id", DEFAULT_ORDERABLE_COLUMNS),
            "SELECT 1 ORDER BY id ASC"
        );
    }

    #[test]
    fn push_order_by_rejects_sensitive_or_unknown_column() {
        // password 不在白名单 → 回退 id ASC，且忽略攻击者指定的 DESC
        assert_eq!(
            order_sql("-password", DEFAULT_ORDERABLE_COLUMNS),
            "SELECT 1 ORDER BY id ASC"
        );
        assert_eq!(
            order_sql("not_a_col", &["id", "created"]),
            "SELECT 1 ORDER BY id ASC"
        );
    }

    #[test]
    fn push_order_by_rejects_injection_shaped_input() {
        assert_eq!(
            order_sql("id; drop table users", DEFAULT_ORDERABLE_COLUMNS),
            "SELECT 1 ORDER BY id ASC"
        );
    }

    /// **回归守卫**：关键词里的通配符必须按字面量匹配。
    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("abc"), "abc");
        assert_eq!(escape_like("%"), "\\%");
        assert_eq!(escape_like("_"), "\\_");
        assert_eq!(escape_like("50%_off"), "50\\%\\_off");
        // 转义符自身也要转义，否则 `a\` 会把后面的 `%` 吃掉
        assert_eq!(escape_like("a\\"), "a\\\\");
        // 非 ASCII 原样保留
        assert_eq!(escape_like("中文%"), "中文\\%");
    }

    /// **回归守卫**：超大页码不得溢出。
    #[test]
    fn page_offset_saturates_instead_of_overflowing() {
        assert_eq!(page_offset(0, 20), 0);
        assert_eq!(page_offset(1, 20), 0);
        assert_eq!(page_offset(3, 20), 40);
        // 旧实现在这里 debug panic / release 回绕
        assert_eq!(page_offset(u64::MAX, 200), i64::MAX as u64);
    }

    #[test]
    fn pagination_sql_is_well_formed_for_huge_page() {
        let params = ModelListParams {
            page: u64::MAX,
            limit: 200,
            ..Default::default()
        };
        let mut qb = QueryBuilder::<Postgres>::new("SELECT 1");
        params.push_pagination(&mut qb, DEFAULT_ORDERABLE_COLUMNS);
        assert_eq!(
            qb.sql().as_str(),
            format!("SELECT 1 ORDER BY id ASC LIMIT 200 OFFSET {}", i64::MAX)
        );
    }

    #[test]
    fn soft_delete_fragments_are_stable() {
        assert!(SOFT_DELETE_SET.contains("deleted_at"));
        assert!(ACTIVE_BY_ID_WHERE.contains("deleted_at IS NULL"));
        assert_eq!(ACTIVE_WHERE, " WHERE deleted_at IS NULL");
    }
}
