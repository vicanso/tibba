use axum::{extract::State, http::Request, middleware::Next, response::Response};
use axum_client_ip::ClientIp;
use chrono::{Duration, Utc};
use urlencoding::decode;

use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct StatsInfo {
    pub ip: String,
    pub method: String,
    pub route: String,
    pub uri: String,
    pub status: http::StatusCode,
    pub cost: Duration,
    pub processing: i32,
}

pub async fn stats<B>(
    State(state): State<&AppState>,
    ClientIp(ip): ClientIp,
    req: Request<B>,
    next: Next<B>,
) -> Response {
    let start_at = Utc::now();
    state.increase_processing();
    let processing_count = state.get_processing();

    let mut uri = req.uri().to_string();
    // decode成功则替换
    if let Ok(result) = decode(uri.as_str()) {
        uri = result.to_string()
    }
    let method = req.method().to_string();
    let route = req.uri().path().to_string();
    // TODO 获取request body
    // 获取 response body

    let resp = next.run(req).await;
    let info = StatsInfo {
        ip: ip.to_string(),
        method,
        route,
        uri,
        status: resp.status(),
        cost: Utc::now() - start_at,
        processing: processing_count,
    };

    tracing::info!("{:?}", info);

    state.decrease_processing();
    resp
}
