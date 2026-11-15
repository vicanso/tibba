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

//! 应用级任务队列运维路由（挂载于 `/jobs`，全部需 Admin 角色）。
//!
//! - `GET    /jobs/stats`            — 队列深度概览（`pending` / `running` / `dead`）
//! - `GET    /jobs/dead?limit&offset` — 分页列出死信任务
//! - `POST   /jobs/dead/{id}/retry`  — 重放一条死信
//! - `DELETE /jobs/dead/{id}`        — 永久删除一条死信

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tibba_auth::AdminSession;
use tibba_error::Error as BaseError;
use tibba_job::{DeadJob, JobQueue};
use tibba_model::format_datetime;
use utoipa::{IntoParams, OpenApi, ToSchema};

type Result<T> = std::result::Result<T, BaseError>;

/// 死信列表单页默认条数（未传 `limit` 时用）。
const DEAD_LIST_DEFAULT_LIMIT: i64 = 50;
/// 死信列表单页上限，防止一次拉取过多。
const DEAD_LIST_MAX_LIMIT: i64 = 200;

/// 死信列表分页参数。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct DeadListQuery {
    /// 本页条数，缺省 50，收敛至 `[1, 200]`。
    limit: Option<i64>,
    /// 偏移量，缺省 0。
    offset: Option<i64>,
}

/// 队列深度概览响应。
#[derive(Debug, Serialize, ToSchema)]
struct QueueStatsResp {
    /// 待跑条数。
    pending: i64,
    /// 执行中条数。
    running: i64,
    /// 死信条数。
    dead: i64,
}

/// 单条死信任务（对外展示，datetime 已格式化为本地时区字符串）。
#[derive(Debug, Serialize, ToSchema)]
struct DeadJobItem {
    id: i64,
    queue: String,
    job_type: String,
    #[schema(value_type = Object)]
    payload: serde_json::Value,
    attempts: i32,
    max_attempts: i32,
    last_error: Option<String>,
    run_at: String,
    created: String,
}

impl From<DeadJob> for DeadJobItem {
    fn from(d: DeadJob) -> Self {
        Self {
            id: d.id,
            queue: d.queue,
            job_type: d.job_type,
            payload: d.payload.0,
            attempts: d.attempts,
            max_attempts: d.max_attempts,
            last_error: d.last_error,
            run_at: format_datetime(d.run_at),
            created: format_datetime(d.created),
        }
    }
}

/// 死信 id 未命中（已被重放 / 删除 / 本就不存在）时的 404。
fn dead_not_found(id: i64) -> BaseError {
    BaseError::new(format!("dead job not found: id={id}"))
        .with_category("job")
        .with_sub_category("dead_job_not_found")
        .with_status(404)
}

/// `GET /jobs/stats` —— 队列深度概览（Admin）。
#[utoipa::path(
    get,
    path = "/jobs/stats",
    tag = "job",
    responses((status = 200, description = "队列深度统计", body = QueueStatsResp))
)]
async fn queue_stats(
    _admin: AdminSession,
    State(pool): State<&'static PgPool>,
) -> Result<Json<QueueStatsResp>> {
    let stats = JobQueue::new(pool).stats().await?;
    Ok(Json(QueueStatsResp {
        pending: stats.pending,
        running: stats.running,
        dead: stats.dead,
    }))
}

/// `GET /jobs/dead` —— 分页列出死信任务（Admin）。
#[utoipa::path(
    get,
    path = "/jobs/dead",
    tag = "job",
    params(DeadListQuery),
    responses((status = 200, description = "死信任务列表", body = Vec<DeadJobItem>))
)]
async fn list_dead_jobs(
    _admin: AdminSession,
    State(pool): State<&'static PgPool>,
    Query(query): Query<DeadListQuery>,
) -> Result<Json<Vec<DeadJobItem>>> {
    let limit = query
        .limit
        .unwrap_or(DEAD_LIST_DEFAULT_LIMIT)
        .clamp(1, DEAD_LIST_MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);
    let jobs = JobQueue::new(pool).list_dead(limit, offset).await?;
    Ok(Json(jobs.into_iter().map(DeadJobItem::from).collect()))
}

/// `POST /jobs/dead/{id}/retry` —— 重放一条死信（Admin）。命中 204，否则 404。
#[utoipa::path(
    post,
    path = "/jobs/dead/{id}/retry",
    tag = "job",
    params(("id" = i64, Path, description = "死信任务 ID")),
    responses(
        (status = 204, description = "已重新入队"),
        (status = 404, description = "死信任务不存在")
    )
)]
async fn retry_dead_job(
    _admin: AdminSession,
    State(pool): State<&'static PgPool>,
    Path(id): Path<i64>,
) -> Result<StatusCode> {
    if JobQueue::new(pool).retry_dead(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(dead_not_found(id))
    }
}

/// `DELETE /jobs/dead/{id}` —— 永久删除一条死信（Admin）。命中 204，否则 404。
#[utoipa::path(
    delete,
    path = "/jobs/dead/{id}",
    tag = "job",
    params(("id" = i64, Path, description = "死信任务 ID")),
    responses(
        (status = 204, description = "已永久删除"),
        (status = 404, description = "死信任务不存在")
    )
)]
async fn purge_dead_job(
    _admin: AdminSession,
    State(pool): State<&'static PgPool>,
    Path(id): Path<i64>,
) -> Result<StatusCode> {
    if JobQueue::new(pool).purge_dead(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(dead_not_found(id))
    }
}

/// 构造任务队列 admin 路由（由主应用以 `/jobs` 前缀挂载）。
pub fn new_job_router(pool: &'static PgPool) -> Router {
    Router::new()
        .route("/stats", get(queue_stats))
        .route("/dead", get(list_dead_jobs))
        .route("/dead/{id}/retry", post(retry_dead_job))
        .route("/dead/{id}", delete(purge_dead_job))
        .with_state(pool)
}

#[derive(OpenApi)]
#[openapi(
    paths(queue_stats, list_dead_jobs, retry_dead_job, purge_dead_job),
    components(schemas(QueueStatsResp, DeadJobItem)),
    tags((name = "job", description = "后台任务队列状态与死信运维"))
)]
struct JobApiDoc;

/// 返回 job 路由的 OpenAPI 文档片段，供主 crate 合并进全局文档。
pub fn openapi() -> utoipa::openapi::OpenApi {
    JobApiDoc::openapi()
}
