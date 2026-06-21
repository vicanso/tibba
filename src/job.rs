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

//! 应用级异步任务 handler 注册。
//!
//! 这里把「注册赠积分」从原先的 fire-and-forget（`let _ = recharge().await`，失败即丢）
//! 改为入队的 `gift_points` 任务：注册流程只负责入队，真正充值由 worker 执行，
//! **失败会自动重试**（多次失败转死信），不再静默丢分。

use crate::config::must_get_webhook_config;
use crate::sql::get_db_pool;
use axum::Json;
use axum::Router;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tibba_error::Error as BaseError;
use tibba_job::{BoxFuture, DeadJob, JobContext, JobHandler, JobQueue, register_handler};
use tibba_model::format_datetime;
use tibba_model_token::{RECHARGE_SOURCE_GIFT, TokenRechargeInsertParams, TokenService};
use tibba_session::AdminSession;
use tibba_webhook::{WebhookDelivery, WebhookHandler};

type Result<T> = std::result::Result<T, BaseError>;

/// 任务类型名：注册赠积分。入队与 handler 须用同一常量，避免拼写漂移。
pub const JOB_GIFT_POINTS: &str = "gift_points";

/// 注册赠送的积分数（与原 on_register 内联逻辑一致）。
const GIFT_AMOUNT: i64 = 1_000_000;

/// `gift_points` 任务处理器：给指定用户充值赠送积分。
///
/// 幂等性说明：当前按「至少一次」语义实现；若需严格一次，可在 payload 带幂等键并在
/// 充值前查重。注册赠分多发的风险可接受，MVP 暂不加额外去重。
struct GiftPointsHandler;

impl JobHandler for GiftPointsHandler {
    fn job_type(&self) -> &'static str {
        JOB_GIFT_POINTS
    }

    fn handle(&self, ctx: JobContext) -> BoxFuture<'_, std::result::Result<(), BaseError>> {
        Box::pin(async move {
            let user_id = ctx
                .payload
                .get("user_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| {
                    BaseError::new("gift_points: payload missing user_id")
                        .with_category("job")
                        .with_sub_category("bad_payload")
                })?;

            TokenService::recharge(
                get_db_pool(),
                TokenRechargeInsertParams {
                    user_id,
                    amount: GIFT_AMOUNT,
                    source: RECHARGE_SOURCE_GIFT,
                    remark: Some("注册赠送".to_string()),
                    ..Default::default()
                },
            )
            .await?;
            Ok(())
        })
    }
}

/// 注册所有应用级任务 handler。须在 `tibba_job::start` 之前调用。
pub fn register_job_handlers() -> Result<()> {
    register_handler(Arc::new(GiftPointsHandler));
    // 出站 webhook：用配置的签名密钥构建 handler（密钥为空则投递不签名）
    let webhook_handler = WebhookHandler::builder()
        .with_secret(must_get_webhook_config().secret.clone())
        .build()?;
    register_handler(Arc::new(webhook_handler));
    Ok(())
}

// ── 任务队列 admin 路由（挂载于 `/jobs`，全部需 Admin 角色）─────────────────
//
// - GET    /jobs/stats              队列深度概览（pending / running / dead）
// - GET    /jobs/dead?limit&offset  分页列出死信任务
// - POST   /jobs/dead/{id}/retry    重放一条死信
// - DELETE /jobs/dead/{id}          永久删除一条死信
//
// 队列深度同时由 worker 侧周期性上报为 `job_queue_depth` gauge 供 Prometheus 抓取；
// 这些端点用于人工排查与处置死信，与 `/features` 同属应用级 admin 管理面。

/// 死信列表单页默认条数（未传 `limit` 时用）。
const DEAD_LIST_DEFAULT_LIMIT: i64 = 50;
/// 死信列表单页上限，防止一次拉取过多。
const DEAD_LIST_MAX_LIMIT: i64 = 200;

/// 死信列表分页参数。
#[derive(Debug, Deserialize)]
struct DeadListQuery {
    /// 本页条数，缺省 50，收敛至 [1, 200]。
    limit: Option<i64>,
    /// 偏移量，缺省 0。
    offset: Option<i64>,
}

/// 队列深度概览响应。
#[derive(Debug, Serialize)]
struct QueueStatsResp {
    /// 待跑条数。
    pending: i64,
    /// 执行中条数。
    running: i64,
    /// 死信条数。
    dead: i64,
}

/// 单条死信任务（对外展示，datetime 已格式化为本地时区字符串）。
#[derive(Debug, Serialize)]
struct DeadJobItem {
    id: i64,
    queue: String,
    job_type: String,
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
async fn queue_stats(_admin: AdminSession) -> Result<Json<QueueStatsResp>> {
    let stats = JobQueue::new(get_db_pool()).stats().await?;
    Ok(Json(QueueStatsResp {
        pending: stats.pending,
        running: stats.running,
        dead: stats.dead,
    }))
}

/// `GET /jobs/dead` —— 分页列出死信任务（Admin）。
async fn list_dead_jobs(
    _admin: AdminSession,
    Query(query): Query<DeadListQuery>,
) -> Result<Json<Vec<DeadJobItem>>> {
    let limit = query
        .limit
        .unwrap_or(DEAD_LIST_DEFAULT_LIMIT)
        .clamp(1, DEAD_LIST_MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);
    let jobs = JobQueue::new(get_db_pool()).list_dead(limit, offset).await?;
    Ok(Json(jobs.into_iter().map(DeadJobItem::from).collect()))
}

/// `POST /jobs/dead/{id}/retry` —— 重放一条死信（Admin）。命中 204，否则 404。
async fn retry_dead_job(_admin: AdminSession, Path(id): Path<i64>) -> Result<StatusCode> {
    if JobQueue::new(get_db_pool()).retry_dead(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(dead_not_found(id))
    }
}

/// `DELETE /jobs/dead/{id}` —— 永久删除一条死信（Admin）。命中 204，否则 404。
async fn purge_dead_job(_admin: AdminSession, Path(id): Path<i64>) -> Result<StatusCode> {
    if JobQueue::new(get_db_pool()).purge_dead(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(dead_not_found(id))
    }
}

/// `POST /jobs/webhooks/test` 请求体：入队一条测试 webhook 投递。
#[derive(Debug, Deserialize)]
struct WebhookTestReq {
    /// 目标 URL（接收方 endpoint）。
    url: String,
    /// 事件类型，缺省 `test`。
    #[serde(default = "default_webhook_event")]
    event: String,
    /// 业务数据，缺省 null。
    #[serde(default)]
    payload: serde_json::Value,
}

fn default_webhook_event() -> String {
    "test".to_string()
}

/// 入队 webhook 测试投递的响应：返回新任务 id（可据此到 `/jobs/dead` 排查投递结果）。
#[derive(Debug, Serialize)]
struct WebhookTestResp {
    job_id: i64,
}

/// `POST /jobs/webhooks/test` —— 入队一条测试 webhook 投递（Admin）。
///
/// 便于联调接收方与验签：worker 取出后签名 + POST。注意会让服务端向请求体给定的
/// URL 发起出站 POST（与 httpstat 探测同类的管理员可触发出站请求），故仅 Admin 可用。
async fn enqueue_webhook_test(
    _admin: AdminSession,
    Json(req): Json<WebhookTestReq>,
) -> Result<Json<WebhookTestResp>> {
    let delivery = WebhookDelivery::new(req.url, req.event, req.payload);
    let job_id = tibba_webhook::enqueue(get_db_pool(), &delivery).await?;
    Ok(Json(WebhookTestResp { job_id }))
}

/// 构造任务队列 admin 路由（由 `router.rs` 以 `/jobs` 前缀挂载）。
pub fn new_job_router() -> Router {
    Router::new()
        .route("/stats", get(queue_stats))
        .route("/dead", get(list_dead_jobs))
        .route("/dead/{id}/retry", post(retry_dead_job))
        .route("/dead/{id}", delete(purge_dead_job))
        .route("/webhooks/test", post(enqueue_webhook_test))
}
