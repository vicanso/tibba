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

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use scopeguard::defer;
use std::sync::Arc;
use tibba_state::{CTX, Context};
use tibba_util::{
    get_device_id_from_cookie, set_header_if_not_exist, set_no_cache_if_not_exist, uuid,
};
use tracing::debug;

/// Entry middleware that sets up request context and handles response headers
///
/// This middleware:
/// 1. Extracts device ID from cookies
/// 2. Generates a unique trace ID
/// 3. Creates and manages request context
/// 4. Sets response headers for caching and tracing
///
/// The middleware ensures each request has:
/// - A unique trace ID for request tracking
/// - Proper cache control headers
/// - Access to device identification
/// - Request-scoped context
pub async fn entry(jar: CookieJar, req: Request, next: Next) -> Response {
    // Log middleware entry
    debug!(category = "middleware", "--> entry");
    // Ensure exit logging happens even if processing panics
    defer!(debug!(category = "middleware", "<-- entry"););

    // Extract device ID from cookies for user/device tracking
    let device_id = get_device_id_from_cookie(&jar);
    // Generate unique trace ID for request tracking
    let trace_id = uuid();
    // Create new context with device and trace information
    let ctx = Context::new(&device_id, &trace_id);

    // Process request within context scope
    let mut res = CTX
        .scope(Arc::new(ctx), async { next.run(req).await })
        .await;

    // Add response headers
    let headers = res.headers_mut();
    // Ensure cache control headers are set
    set_no_cache_if_not_exist(headers);
    // Add trace ID header for request tracking
    let _ = set_header_if_not_exist(headers, "X-Trace-Id", &trace_id);

    res
}
