// Copyright 2026 Tree xie.
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

use super::LOG_TARGET;
use axum::extract::Request;
use axum::extract::State;
use axum::middleware::Next;
use axum::response::Response;
use scopeguard::defer;
use std::time::Duration;
use tibba_runtime::CTX;
use tokio::time::sleep;
use tracing::debug;

/// Parameters for configuring the wait middleware
/// Controls the waiting behavior after request processing
#[derive(Debug, Clone, Default)]
pub struct WaitParams {
    // Duration to wait in milliseconds
    wait: Duration,
    // If true, only wait when an error response occurs (status >= 400)
    only_error_occurred: bool,
}

impl WaitParams {
    /// Creates a new WaitParams with the specified wait duration in milliseconds.
    pub fn new(ms: u64) -> Self {
        Self {
            wait: Duration::from_millis(ms),
            ..Default::default()
        }
    }

    /// Only wait when the response status is >= 400.
    #[must_use]
    pub fn only_on_error(mut self) -> Self {
        self.only_error_occurred = true;
        self
    }
}

/// Middleware that adds a configurable delay after request processing
///
/// This middleware can be useful for:
/// - Rate limiting
/// - Simulating network latency
/// - Preventing timing attacks
/// - Ensuring minimum response times
///
/// # Arguments
/// * `State(params)` - Wait configuration parameters
/// * `req` - The incoming request
/// * `next` - The next middleware in the chain
pub async fn wait(State(params): State<WaitParams>, req: Request, next: Next) -> Response {
    // Log middleware entry
    debug!(target: LOG_TARGET, "--> wait");
    // Ensure exit logging happens even if processing panics
    defer!(debug!(target: LOG_TARGET, "<-- wait"););

    // Process the request through the middleware chain
    let res = next.run(req).await;

    // Check if we should wait based on error condition
    if params.only_error_occurred && res.status().as_u16() < 400 {
        return res;
    }

    // Calculate remaining time to wait；若外层未初始化 CTX 则回退为 0 耗时
    let elapsed = CTX.try_with(|ctx| ctx.elapsed()).unwrap_or_default();
    let remaining_wait = params.wait.saturating_sub(elapsed);

    // Only wait if the remaining time is significant (>= 10ms)
    if remaining_wait.as_millis() >= 10 {
        sleep(remaining_wait).await
    }

    res
}
