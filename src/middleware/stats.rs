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

use crate::util::{clone_value_from_context, get_account_from_context, DEVICE_ID, TRACE_ID};
use crate::{
    error::{HTTPError, HTTPResult},
    state::AppState,
    util::read_http_body,
};

#[derive(Debug, Clone)]
pub struct StatsInfo {
    pub device_id: String,
    pub trace_id: String,
    pub account: String,
    pub ip: String,
    pub method: String,
    pub route: String,
    pub uri: String,
    pub status: http::StatusCode,
    pub cost: Duration,
    pub processing: i32,
    pub request_body_size: usize,
}

pub async fn stats(
    State(state): State<&AppState>,
    ClientIp(ip): ClientIp,
    mut req: Request<Body>,
    next: Next<Body>,
) -> HTTPResult<Response> {
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
        let bytes = read_http_body(body).await?;
        request_body_size = bytes.len();
        req = Request::from_parts(parts, Body::from(bytes));
    }
    let trace_id = TRACE_ID.with(clone_value_from_context);
    let device_id = DEVICE_ID.with(clone_value_from_context);

    // TODO
    // 获取 response body
    let resp = next.run(req).await;
    // account 在获取session后才能获取
    // 而task local的值已回收，因此只能从extensions中获取
    let account = get_account_from_context(resp.extensions());

    let info = StatsInfo {
        device_id,
        trace_id,
        account,
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
        deviceId = info.device_id,
        traceId = info.trace_id,
        account = info.account,
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
