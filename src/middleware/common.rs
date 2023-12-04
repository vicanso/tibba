use crate::task_local::*;
use axum::{body::Body, http::Request, middleware::Next, response::Response};
use chrono::Utc;
use std::time::Duration;
use tokio::time::sleep;

async fn wait(ms: i64, only_err_occurred: bool, req: Request<Body>, next: Next) -> Response {
    let resp = next.run(req).await;
    // 如果仅出错时等待
    if only_err_occurred && resp.status().as_u16() < 400 {
        return resp;
    }
    let started_at = STARTED_AT.with(clone_value_from_task_local);

    let offset = ms - (Utc::now().timestamp_millis() - started_at);
    // 如果处理时长与等待时长还有 x ms的差距，则等待
    if offset > 10 {
        sleep(Duration::from_millis(offset as u64)).await
    }
    resp
}

/// 如果响应处理时间少于1秒，则等待
pub async fn wait1s(req: Request<Body>, next: Next) -> Response {
    wait(1000, false, req, next).await
}

/// 如果响应处理时间少于2秒，则等待
pub async fn wait2s(req: Request<Body>, next: Next) -> Response {
    wait(2000, false, req, next).await
}

/// 如果响应处理时间少于3秒，则等待
pub async fn wait3s(req: Request<Body>, next: Next) -> Response {
    wait(3000, false, req, next).await
}
