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
use axum::response::Response;
use scopeguard::defer;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_error::{Error, new_error_with_category, new_http_error};
use tibba_state::CTX;
use tokio::time::sleep;
use tracing::debug;

type Result<T> = std::result::Result<T, Error>;

/// Parameters for configuring the wait middleware
/// Controls the waiting behavior after request processing
#[derive(Debug, Clone, Default)]
pub struct WaitParams {
    // Duration to wait in milliseconds
    pub ms: u64,
    // If true, only wait when an error response occurs (status >= 400)
    pub only_error_occurred: bool,
}

impl WaitParams {
    /// Creates a new WaitParams instance with specified wait duration
    /// and default settings for other parameters
    pub fn new(ms: u64) -> Self {
        Self {
            ms,
            ..Default::default()
        }
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
    debug!(category = "middleware", "--> wait");
    // Ensure exit logging happens even if processing panics
    defer!(debug!(category = "middleware", "<-- wait"););

    // Process the request through the middleware chain
    let res = next.run(req).await;

    // Check if we should wait based on error condition
    if params.only_error_occurred && res.status().as_u16() < 400 {
        return res;
    }

    // Calculate remaining time to wait
    let elapsed = CTX.get().elapsed().as_millis();
    let offset = params.ms - elapsed as u64;

    // Only wait if the remaining time is significant (>= 10ms)
    if offset >= 10 {
        sleep(Duration::from_millis(offset)).await
    }

    res
}

/// Middleware to validate captcha tokens in incoming requests
///
/// # Arguments
/// * `magic_code` - Special code that can bypass normal captcha validation (for testing)
/// * `cache` - Redis cache instance for storing/retrieving captcha codes
/// * `req` - The incoming HTTP request
/// * `next` - The next middleware handler
///
/// # Format
/// The X-Captcha header should contain a colon-separated string with 3 parts:
/// `prefix:key:code` where:
/// - prefix: identifier for the captcha type
/// - key: unique key to look up the stored captcha code
/// - code: the actual captcha code to validate
pub async fn validate_captcha(
    State(magic_code): State<String>,
    State(cache): State<&'static RedisCache>,
    req: Request,
    next: Next,
) -> Result<Response> {
    // Category name for error handling
    let category = "captcha";

    // Extract and parse the X-Captcha header
    let value = req
        .headers()
        .get("X-Captcha")
        .ok_or(new_error_with_category(
            "captcha is required".to_string(),
            category.to_string(),
        ))?
        .to_str()
        .map_err(|err| new_error_with_category(err.to_string(), category.to_string()))?;

    // Split the header value into its components
    let arr: Vec<&str> = value.split(':').collect();

    // Validate the header format
    if arr.len() != 3 {
        return Err(new_error_with_category(
            "captcha parameter is invalid".to_string(),
            category.to_string(),
        ));
    }

    // Check if this is a mock request using the magic code
    let is_mock = !magic_code.is_empty() && arr[2] == magic_code;

    // For non-mock requests, validate the captcha code against cache
    if !is_mock {
        // Retrieve and delete the stored code from cache using the key (arr[1])
        let code: String = cache.get_del(arr[1]).await?;

        // Compare the provided code against the stored code
        if code != arr[2] {
            let he = new_http_error("captcha input error".to_string())
                .with_category(category.to_string())
                .with_code("mismatching".to_string());
            return Err(he.into());
        }
    }

    // If validation passes, continue with the request
    let resp = next.run(req).await;
    Ok(resp)
}
