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
use axum::extract::State;
use axum::middleware::Next;
use axum::response::IntoResponse;
use scopeguard::defer;
use tibba_error::{Error, new_exception_error_with_status};
use tibba_state::AppState;
use tracing::debug;

// Custom Result type that uses the application's Error type
type Result<T> = std::result::Result<T, Error>;

/// Middleware that implements concurrent request processing limits
///
/// This middleware:
/// 1. Tracks number of concurrent requests being processed
/// 2. Enforces a maximum limit on concurrent requests
/// 3. Returns 429 Too Many Requests when limit is exceeded
/// 4. Properly decrements counter when request processing completes
///
/// # Arguments
/// * `State(state)` - Application state containing limit configuration
/// * `req` - The incoming request
/// * `next` - The next middleware in the chain
pub async fn processing_limit(
    State(state): State<&AppState>,
    req: Request,
    next: Next,
) -> Result<impl IntoResponse> {
    // Log middleware entry
    debug!(category = "middleware", "--> processing_limit");
    // Ensure exit logging happens even if processing panics
    defer!(debug!(category = "middleware", "<-- processing_limit"););

    // Get configured processing limit from app state
    let limit = state.get_processing_limit();

    // If limit is negative, processing is unlimited
    if limit < 0 {
        let res = next.run(req).await;
        return Ok(res);
    }

    // Increment processing counter and get new count
    let count = state.inc_processing() + 1;

    // Check if processing limit has been exceeded
    if count > limit {
        // Decrement counter since request won't be processed
        state.dec_processing();
        // Return 429 Too Many Requests error
        return Err(new_exception_error_with_status(
            "Too many requests".to_string(),
            429,
        ));
    }

    // Process the request
    let res = next.run(req).await;
    // Decrement processing counter after request completes
    state.dec_processing();

    Ok(res)
}
