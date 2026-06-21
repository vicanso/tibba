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

//! 行级多租户演示：基于 `tenant_notes` 表的租户隔离 CRUD。
//!
//! 演示要点（隔离模式，业务可照搬到自己的多租户表）：
//! - 当前租户由 [`tibba_tenant::TenantId`] 提取器从 `X-Tenant-Id` 头 / 请求扩展解析；
//! - 写入恒带 `tenant_id`，读取 / 删除恒 `WHERE tenant_id = $T`；
//! - 单行读写用 `id = $1 AND tenant_id = $2` 做**纵深防御**：即便猜到别的租户的行 id，
//!   也读不到 / 删不掉，避免越权（IDOR）。
//!
//! 挂载于 `/tenant`（见 `router.rs`），所有端点都需带租户标识。

use crate::sql::get_db_pool;
use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use time::PrimitiveDateTime;
use tibba_error::Error as BaseError;
use tibba_model::format_datetime;
use tibba_tenant::TenantId;

type Result<T> = std::result::Result<T, BaseError>;

const ERROR_CATEGORY: &str = "tenant_demo";

/// 列表单次返回上限。
const LIST_LIMIT: i64 = 100;
/// 便签内容长度上限。
const MAX_CONTENT_LEN: usize = 10_000;

/// 本模块内部错误，统一转换为 `tibba_error::Error`。
#[derive(Debug, Snafu)]
enum Error {
    /// 数据库操作失败。
    #[snafu(display("sqlx: {source}"))]
    Sqlx {
        #[snafu(source(from(sqlx::Error, Box::new)))]
        source: Box<sqlx::Error>,
    },
    /// 目标便签在当前租户下不存在（不存在 / 属于他人，统一 404，不泄露存在性）。
    #[snafu(display("note not found: {id}"))]
    NotFound { id: i64 },
    /// 请求参数非法（内容空或超长）。
    #[snafu(display("{message}"))]
    Invalid { message: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Sqlx { source } => BaseError::new(source).with_exception(true),
            Error::NotFound { id } => BaseError::new(format!("note not found: {id}"))
                .with_sub_category("not_found")
                .with_status(404)
                .with_exception(false),
            Error::Invalid { message } => BaseError::new(message)
                .with_sub_category("invalid")
                .with_status(400)
                .with_exception(false),
        };
        err.with_category(ERROR_CATEGORY)
    }
}

/// `tenant_notes` 行。
#[derive(Debug, sqlx::FromRow)]
struct NoteRow {
    id: i64,
    tenant_id: String,
    content: String,
    created: PrimitiveDateTime,
}

/// 便签 API 响应（datetime 已格式化为本地时区字符串）。
#[derive(Debug, Serialize)]
struct NoteResp {
    id: i64,
    tenant_id: String,
    content: String,
    created: String,
}

impl From<NoteRow> for NoteResp {
    fn from(row: NoteRow) -> Self {
        Self {
            id: row.id,
            tenant_id: row.tenant_id,
            content: row.content,
            created: format_datetime(row.created),
        }
    }
}

/// 创建便签请求体。
#[derive(Debug, Deserialize)]
struct CreateNoteReq {
    content: String,
}

/// `POST /tenant/notes` —— 在当前租户下创建一条便签。
async fn create_note(tenant: TenantId, Json(req): Json<CreateNoteReq>) -> Result<Json<NoteResp>> {
    let content = req.content.trim();
    if content.is_empty() {
        return Err(Error::Invalid {
            message: "content is empty".to_string(),
        }
        .into());
    }
    if content.len() > MAX_CONTENT_LEN {
        return Err(Error::Invalid {
            message: format!("content too long (max {MAX_CONTENT_LEN})"),
        }
        .into());
    }
    // 写入恒带 tenant_id：数据自带隔离键
    let row: NoteRow = sqlx::query_as(
        r#"INSERT INTO tenant_notes (tenant_id, content)
           VALUES ($1, $2)
           RETURNING id, tenant_id, content, created"#,
    )
    .bind(tenant.as_str())
    .bind(content)
    .fetch_one(get_db_pool())
    .await
    .context(SqlxSnafu)?;
    Ok(Json(row.into()))
}

/// `GET /tenant/notes` —— 列出当前租户的便签（最近优先）。
async fn list_notes(tenant: TenantId) -> Result<Json<Vec<NoteResp>>> {
    // 读取恒按 tenant_id 过滤：只可能看到自己租户的数据
    let rows: Vec<NoteRow> = sqlx::query_as(
        r#"SELECT id, tenant_id, content, created
             FROM tenant_notes
            WHERE tenant_id = $1
            ORDER BY id DESC
            LIMIT $2"#,
    )
    .bind(tenant.as_str())
    .bind(LIST_LIMIT)
    .fetch_all(get_db_pool())
    .await
    .context(SqlxSnafu)?;
    Ok(Json(rows.into_iter().map(NoteResp::from).collect()))
}

/// `GET /tenant/notes/{id}` —— 读取本租户的某条便签。
async fn get_note(tenant: TenantId, Path(id): Path<i64>) -> Result<Json<NoteResp>> {
    // 纵深防御：id + tenant_id 双条件，猜到他人 id 也读不到
    let row: Option<NoteRow> = sqlx::query_as(
        r#"SELECT id, tenant_id, content, created
             FROM tenant_notes
            WHERE id = $1 AND tenant_id = $2"#,
    )
    .bind(id)
    .bind(tenant.as_str())
    .fetch_optional(get_db_pool())
    .await
    .context(SqlxSnafu)?;
    let row = row.ok_or(Error::NotFound { id })?;
    Ok(Json(row.into()))
}

/// `DELETE /tenant/notes/{id}` —— 删除本租户的某条便签（命中 204，否则 404）。
async fn delete_note(tenant: TenantId, Path(id): Path<i64>) -> Result<StatusCode> {
    // 同样 id + tenant_id 双条件：删不掉别的租户的行
    let result = sqlx::query("DELETE FROM tenant_notes WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(tenant.as_str())
        .execute(get_db_pool())
        .await
        .context(SqlxSnafu)?;
    if result.rows_affected() == 0 {
        return Err(Error::NotFound { id }.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 构造多租户演示路由（由 `router.rs` 以 `/tenant` 前缀挂载）。
pub fn new_tenant_router() -> Router {
    Router::new()
        .route("/notes", post(create_note).get(list_notes))
        .route("/notes/{id}", get(get_note).delete(delete_note))
}
