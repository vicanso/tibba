use axum::{
    body::{Body, Bytes},
    extract::State,
    http::Request,
    middleware::Next,
    response::Response,
};
use axum_client_ip::ClientIp;
use chrono::{Duration, Utc};
use http::Method;
use tracing::{event, Level};
use urlencoding::decode;

use crate::util::get_context;
use crate::{error::HTTPError, state::AppState};

#[derive(Debug, Clone)]
pub struct StatsInfo {
    pub trace_id: String,
    pub ip: String,
    pub method: String,
    pub route: String,
    pub uri: String,
    pub status: http::StatusCode,
    pub cost: Duration,
    pub processing: i32,
    pub request_body_size: usize,
}

async fn read_buffer<B>(body: B) -> Result<Bytes, HTTPError>
where
    B: axum::body::HttpBody<Data = Bytes>,
    B::Error: std::fmt::Display,
{
    let bytes = match hyper::body::to_bytes(body).await {
        Ok(bytes) => bytes,
        Err(err) => {
            let msg = format!("failed to read body: {}", err);
            return Err(HTTPError::new(msg.as_str()));
        }
    };
    Ok(bytes)
}

pub async fn stats(
    State(state): State<&AppState>,
    ClientIp(ip): ClientIp,
    mut req: Request<Body>,
    next: Next<Body>,
) -> Result<Response, HTTPError> {
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
    let mut request_body_size = 0;
    if [Method::POST, Method::PATCH, Method::PUT].contains(req.method()) {
        let (parts, body) = req.into_parts();
        let bytes = read_buffer(body).await?;
        request_body_size = bytes.len();
        req = Request::from_parts(parts, Body::from(bytes));
    }

    let mut ctx = get_context(req.extensions());
    // TODO
    // 获取 response body
    let resp = next.run(req).await;

    // account 在获取session后才能获取
    ctx.account = get_context(resp.extensions()).account;

    let info = StatsInfo {
        trace_id: ctx.trace_id,
        ip: ip.to_string(),
        method,
        route,
        uri,
        status: resp.status(),
        cost: Utc::now() - start_at,
        processing: processing_count,
        request_body_size,
    };

    event!(
        Level::INFO,
        traceId = info.trace_id,
        ip = info.ip,
        method = info.method,
        uri = info.uri,
        status = info.status.as_u16(),
        cost = info.cost.num_milliseconds(),
        processing = info.processing,
        requestBodySize = info.request_body_size,
    );

    state.decrease_processing();
    Ok(resp)
}
