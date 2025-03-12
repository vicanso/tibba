// Copyright 2025 Tree xie.
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

use axum::body::Body;
use axum::extract::Request;
use axum::extract::State;
use axum::middleware::Next;
use axum::response::Response;
use axum_client_ip::InsecureClientIp;
use scopeguard::defer;
use tibba_error::{Error, new_exception_error_with_status};
use tibba_state::{AppState, CTX};
use tibba_util::{get_header_value, json_get, read_http_body};
use tracing::{debug, info};
use urlencoding::decode;

type Result<T> = std::result::Result<T, Error>;

pub async fn stats(
    State(state): State<&AppState>,
    InsecureClientIp(ip): InsecureClientIp,
    req: Request,
    next: Next,
) -> Result<Response> {
    debug!(category = "middleware", "--> stats");
    defer!(debug!(category = "middleware", "<-- stats"););
    let mut uri = req.uri().to_string();
    if let Ok(result) = decode(&uri) {
        uri = result.to_string()
    }
    let method = req.method().to_string();
    let x_forwarded_for = get_header_value(req.headers(), "X-Forwarded-For");
    let referrer = get_header_value(req.headers(), "Referer");
    let mut res = next.run(req).await;
    let status = res.status().as_u16();
    let ctx = CTX.get();
    let mut message = None;
    if status >= 400 {
        let (parts, body) = res.into_parts();
        let data = read_http_body(body)
            .await
            .map_err(|e| new_exception_error_with_status(e.to_string(), 500))?;
        // TODO get error message
        message = Some(json_get(&data, "message"));
        res = Response::from_parts(parts, Body::from(data));
    }

    // TODO add more info for error
    info!(
        category = "access",
        device_id = ctx.device_id,
        trace_id = ctx.trace_id,
        ip = ip.to_string(),
        processing = state.get_processing(),
        x_forwarded_for,
        referrer,
        method,
        uri,
        status,
        elapsed = ctx.elapsed().as_millis(),
        error = message,
    );

    Ok(res)
}
