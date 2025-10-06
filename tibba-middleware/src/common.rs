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

use super::Error;
use axum::extract::Request;
use axum::extract::State;
use axum::middleware::Next;
use axum::response::Response;
use scopeguard::defer;
use std::time::Duration;
use tibba_cache::RedisCache;
use tibba_state::CTX;
use tokio::time::sleep;
use tracing::debug;

type Result<T> = std::result::Result<T, tibba_error::Error>;

/// Parameters for configuring the wait middleware
/// Controls the waiting behavior after request processing
#[derive(Debug, Clone, Default)]
pub struct WaitParams {
    // Duration to wait in milliseconds
    pub wait: Duration,
    // If true, only wait when an error response occurs (status >= 400)
    pub only_error_occurred: bool,
}

impl WaitParams {
    /// Creates a new WaitParams instance with specified wait duration
    /// and default settings for other parameters
    pub fn new(ms: u64) -> Self {
        Self {
            wait: Duration::from_millis(ms),
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
    let elapsed = CTX.get().elapsed();
    let remaining_wait = params.wait.saturating_sub(elapsed);

    // Only wait if the remaining time is significant (>= 10ms)
    if remaining_wait.as_millis() >= 10 {
        sleep(remaining_wait).await
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
/// `key:code` where:
/// - key: unique key to look up the stored captcha code
/// - code: the actual captcha code to validate
pub async fn validate_captcha(
    State((magic_code, cache)): State<(String, &'static RedisCache)>,
    req: Request,
    next: Next,
) -> Result<Response> {
    // Category name for error handling
    let category = "captcha";

    // Extract and parse the X-Captcha header
    let value = req
        .headers()
        .get("X-Captcha")
        .ok_or(Error::Common {
            message: "captcha is required".to_string(),
            category: category.to_string(),
        })?
        .to_str()
        .map_err(|err| Error::Common {
            message: err.to_string(),
            category: category.to_string(),
        })?;

    let (key, user_code) = value.split_once(':').ok_or_else(|| Error::Common {
        message: "captcha parameter is invalid, expect 'key:code'".to_string(),
        category: category.to_string(),
    })?;

    // Check if this is a mock request using the magic code
    let is_mock = !magic_code.is_empty() && user_code == magic_code;

    // For non-mock requests, validate the captcha code against cache
    if !is_mock {
        // Retrieve and delete the stored code from cache using the key (arr[1])
        let code: Option<String> = cache.get_del(key).await?;
        let Some(code) = code else {
            return Err(Error::Common {
                message: "captcha is expired".to_string(),
                category: category.to_string(),
            }
            .into());
        };

        // Compare the provided code against the stored code
        if code != user_code {
            return Err(Error::Common {
                message: "captcha is invalid".to_string(),
                category: category.to_string(),
            }
            .into());
        }
    }

    // If validation passes, continue with the request
    let res = next.run(req).await;
    Ok(res)
}
