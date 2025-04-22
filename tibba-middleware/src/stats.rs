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
use axum_client_ip::ClientIp;
use scopeguard::defer;
use tibba_error::{Error, new_error};
use tibba_state::{AppState, CTX};
use tibba_util::{get_header_value, json_get, read_http_body};
use tracing::{debug, info};
use urlencoding::decode;

// Custom Result type for error handling
type Result<T> = std::result::Result<T, Error>;

/// Statistics middleware that logs request/response information
///
/// This middleware captures and logs:
/// - Request metadata (URI, method, headers)
/// - Client information (IP, forwarded headers)
/// - Response status and timing
/// - Error details when applicable
/// - Processing statistics
///
/// # Arguments
/// * `State(state)` - Application state for processing stats
/// * `InsecureClientIp(ip)` - Client IP address
/// * `req` - The incoming request
/// * `next` - The next middleware in the chain
pub async fn stats(
    State(state): State<&AppState>,
    ClientIp(ip): ClientIp,
    req: Request,
    next: Next,
) -> Result<Response> {
    // Log middleware entry
    debug!(category = "middleware", "--> stats");
    // Ensure exit logging happens even if processing panics
    defer!(debug!(category = "middleware", "<-- stats"););

    // Decode URI for logging (handles URL-encoded characters)
    let mut uri = req.uri().to_string();
    if let Ok(result) = decode(&uri) {
        uri = result.to_string()
    }

    // Collect request information
    let method = req.method().to_string();
    let x_forwarded_for = get_header_value(req.headers(), "X-Forwarded-For");
    let referrer = get_header_value(req.headers(), "Referer");

    // Process the request
    let mut res = next.run(req).await;
    let status = res.status().as_u16();
    let ctx = CTX.get();

    // Extract error message for 4xx/5xx responses
    let mut message = None;
    if status >= 400 {
        // Decompose response to read body
        let (parts, body) = res.into_parts();
        let data = read_http_body(body)
            .await
            .map_err(|e| new_error(&e.to_string()).with_status(500))?;
        // Extract error message from response body
        message = Some(json_get(&data, "message"));
        // Reconstruct response
        res = Response::from_parts(parts, Body::from(data));
    }

    // Log comprehensive request/response information
    info!(
        category = "access",
        device_id = ctx.device_id,           // Device identification
        trace_id = ctx.trace_id,             // Request trace ID
        ip = ip.to_string(),                 // Client IP
        processing = state.get_processing(), // Current processing count
        x_forwarded_for,                     // Forwarded IP chain
        referrer,                            // Request referrer
        method,                              // HTTP method
        uri,                                 // Request URI
        status,                              // Response status code
        elapsed = ctx.elapsed().as_millis(), // Request processing time
        error = message,                     // Error message if any
    );

    Ok(res)
}
