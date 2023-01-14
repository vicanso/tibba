use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};
use axum_client_ip::ClientIp;
use chrono::Utc;
use http::Method;
use tracing::{event, Level};
use urlencoding::decode;

use crate::{
    error::HTTPResult,
    state::AppState,
    util::{
        clone_value_from_context, get_account_from_context, get_header_value, json_get,
        read_http_body, DEVICE_ID, STARTED_AT, TRACE_ID,
    },
};

pub async fn access_log(
    State(state): State<&AppState>,
    ClientIp(ip): ClientIp,
    mut req: Request<Body>,
    next: Next<Body>,
) -> HTTPResult<Response<Body>> {
    let start_at = STARTED_AT.with(clone_value_from_context);
    state.increase_processing();
    let processing_count = state.get_processing();

    let mut uri = req.uri().to_string();
    // decode成功则替换
    if let Ok(result) = decode(uri.as_str()) {
        uri = result.to_string()
    }
    let method = req.method().to_string();
    let x_forwarded_for = get_header_value(req.headers(), "X-Forwarded-For");
    let mut request_body_size = 0;

    // 获取请求数据
    if [Method::POST, Method::PATCH, Method::PUT].contains(req.method()) {
        let (parts, body) = req.into_parts();
        let bytes = read_http_body(body).await?;
        request_body_size = bytes.len();
        req = Request::from_parts(parts, Body::from(bytes));
    }

    let trace_id = TRACE_ID.with(clone_value_from_context);
    let device_id = DEVICE_ID.with(clone_value_from_context);

    let resp = next.run(req).await;
    // account 在获取session后才能获取
    // 而task local的值已回收，因此只能从extensions中获取
    let account = get_account_from_context(resp.extensions());

    let status = resp.status().as_u16();

    let (parts, body) = resp.into_parts();
    let data = read_http_body(body).await?;
    let mut error_message = "".to_string();
    if status >= 400 {
        error_message = std::string::String::from_utf8_lossy(&data).to_string();
    }
    let response_body_size = data.len();
    let res = Response::from_parts(parts, Body::from(data));

    // TODO route 获取（不包括路由参数:id这样)
    // /users/:id 请求/users/123
    // route为 /users/:id

    let cost = Utc::now().timestamp_millis() - start_at;
    event!(
        Level::INFO,
        category = "accessLog",
        deviceId = device_id,
        traceId = trace_id,
        account = account,
        ip = ip.to_string(),
        xForwardedFor = x_forwarded_for,
        method,
        uri,
        status,
        cost,
        processing = processing_count,
        requestBodySize = request_body_size,
        responseBodySize = response_body_size,
    );

    // 出错日志
    if status >= 400 {
        event!(
            Level::ERROR,
            category = "httpError",
            deviceId = device_id,
            traceId = trace_id,
            account = account,
            error = json_get(error_message.as_str(), "message"),
        )
    }

    state.decrease_processing();
    Ok(res)
}
