use axum::http::Method;
use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};
use axum_client_ip::InsecureClientIp;
use chrono::Utc;
use urlencoding::decode;

use crate::error::HttpResult;
use crate::state::AppState;
use crate::util::{get_account_from_context, get_header_value, json_get, read_http_body};
use crate::{task_local::*, tl_error, tl_info};

pub async fn access_log(
    State(state): State<&AppState>,
    InsecureClientIp(ip): InsecureClientIp,
    mut req: Request<Body>,
    next: Next<Body>,
) -> HttpResult<Response<Body>> {
    let start_at = STARTED_AT.with(clone_value_from_task_local);
    state.increase_processing();
    let processing_count = state.get_processing();

    let mut uri = req.uri().to_string();
    // decode成功则替换
    if let Ok(result) = decode(&uri) {
        uri = result.to_string()
    }
    let method = req.method().to_string();
    let x_forwarded_for = get_header_value(req.headers(), "X-Forwarded-For");
    let referrer = get_header_value(req.headers(), "Referer");
    let mut request_body_size = 0;

    // 获取请求数据
    if [Method::POST, Method::PATCH, Method::PUT].contains(req.method()) {
        let (parts, body) = req.into_parts();
        let bytes = read_http_body(body).await?;
        request_body_size = bytes.len();
        req = Request::from_parts(parts, Body::from(bytes));
    }

    let resp = next.run(req).await;
    // account 在获取session后才能获取
    // 而task local的值已回收，因此只能从extensions中获取
    let account = get_account_from_context(resp.extensions());

    let status = resp.status().as_u16();

    let (parts, body) = resp.into_parts();
    let data = read_http_body(body).await?;
    let mut message = "".to_string();
    if status >= 400 {
        message = json_get(&data, "message")
    }
    if message.is_empty() {
        message = std::string::String::from_utf8_lossy(&data).to_string();
    }

    let response_body_size = data.len();
    let res = Response::from_parts(parts, Body::from(data));

    // TODO route 获取（不包括路由参数:id这样)
    // /users/:id 请求/users/123
    // route为 /users/:id

    let cost = Utc::now().timestamp_millis() - start_at;

    tl_info!(
        category = "accessLog",
        account = account,
        ip = ip.to_string(),
        xForwardedFor = x_forwarded_for,
        referrer,
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
        tl_error!(category = "httpError", account = account, error = message);
    }

    state.decrease_processing();
    Ok(res)
}
