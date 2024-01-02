use crate::task_local::*;
use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};
use chrono::Utc;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Clone, Default)]
pub struct WaitParams {
    pub ms: i64,
    pub only_err_occurred: bool,
}

impl WaitParams {
    pub fn new(ms: i64) -> Self {
        Self {
            ms,
            ..Default::default()
        }
    }
}

pub async fn wait(State(params): State<WaitParams>, req: Request<Body>, next: Next) -> Response {
    let resp = next.run(req).await;
    // 如果仅出错时等待
    if params.only_err_occurred && resp.status().as_u16() < 400 {
        return resp;
    }
    let started_at = STARTED_AT.with(clone_value_from_task_local);

    let offset = params.ms - (Utc::now().timestamp_millis() - started_at);
    // 如果处理时长与等待时长还有 x ms的差距，则等待
    if offset > 10 {
        sleep(Duration::from_millis(offset as u64)).await
    }
    resp
}
