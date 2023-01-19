use axum::{http::Request, middleware::Next, response::Response};
use chrono::Utc;
use std::time::Duration;
use tokio::time::sleep;

use crate::util::{clone_value_from_context, STARTED_AT};

async fn wait<B>(ms: i64, only_err_occurred: bool, req: Request<B>, next: Next<B>) -> Response {
    let resp = next.run(req).await;
    // 如果仅出错时等待
    if only_err_occurred && resp.status().as_u16() < 400 {
        return resp;
    }
    let started_at = STARTED_AT.with(clone_value_from_context);

    let offset = ms - (Utc::now().timestamp_millis() - started_at);
    // 如果处理时长与等待时长还有 x ms的差距，则等待
    if offset > 10 {
        sleep(Duration::from_millis(offset as u64)).await
    }
    resp
}

pub async fn wait1s<B>(req: Request<B>, next: Next<B>) -> Response {
    wait(1000, false, req, next).await
}

pub async fn wait2s<B>(req: Request<B>, next: Next<B>) -> Response {
    wait(2000, false, req, next).await
}
pub async fn wait3s<B>(req: Request<B>, next: Next<B>) -> Response {
    wait(3000, false, req, next).await
}
