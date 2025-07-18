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
use axum::response::Response;
use scopeguard::defer;
use std::net::IpAddr;
use std::time::Duration;
use tibba_cache::RedisCache;
// use tibba_error::{Error, new_error};
use super::ClientIp;
use super::Error;
use tibba_state::AppState;
use tracing::debug;

// Custom Result type that uses the application's Error type
type Result<T> = std::result::Result<T, tibba_error::Error>;

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
        return Err(Error::TooManyRequests {
            limit: limit as i64,
            current: count as i64,
        }
        .into());
    }

    // Process the request
    let res = next.run(req).await;
    // Decrement processing counter after request completes
    state.dec_processing();

    Ok(res)
}

/// Type of rate limiting to apply
#[derive(Debug, Clone, Default)]
pub enum LimitType {
    #[default]
    Ip, // Rate limit based on IP address
    Header(String), // Rate limit based on header value
}

/// Configuration parameters for rate limiting middleware
#[derive(Debug, Clone, Default)]
pub struct LimitParams {
    pub limit_type: LimitType, // Type of rate limiting to apply
    pub category: String,      // Category identifier for the limit
    pub max: i64,              // Maximum number of requests allowed
    pub ttl: Duration,         // Time-to-live for the rate limit counter
}

impl LimitParams {
    /// Creates a new LimitParams instance with specified parameters
    ///
    /// # Arguments
    /// * `max` - Maximum number of requests allowed
    /// * `secs` - Duration in seconds for the rate limit window
    /// * `category` - Category identifier for the limit
    pub fn new(max: i64, secs: u64, category: &str) -> Self {
        LimitParams {
            limit_type: LimitType::Ip,
            category: category.to_string(),
            max,
            ttl: Duration::from_secs(secs),
        }
    }
}

/// Generates the cache key and TTL for rate limiting
///
/// # Arguments
/// * `ip` - Client IP address
/// * `params` - Rate limiting parameters
///
/// # Returns
/// Tuple of (cache_key, ttl_duration)
fn get_limit_params(req: &Request, ip: IpAddr, params: &LimitParams) -> (String, Duration) {
    // Generate key based on limit type (currently only IP-based)
    let mut key = match &params.limit_type {
        LimitType::Header(header) => {
            if let Some(value) = req.headers().get(header) {
                value.to_str().unwrap_or_default().to_string()
            } else {
                "".to_string()
            }
        }
        _ => ip.to_string(),
    };
    // Append category to key if specified
    if !params.category.is_empty() {
        key = format!("{}:{key}", params.category);
    }
    // Use default TTL of 5 minutes if none specified
    let mut ttl = params.ttl;
    if ttl.is_zero() {
        ttl = Duration::from_secs(5 * 60);
    }
    (key, ttl)
}

/// Middleware that limits requests only when errors occur
/// Increments counter only for responses with status code >= 400
pub async fn error_limiter(
    ClientIp(ip): ClientIp,
    State(params): State<LimitParams>,
    State(cache): State<&'static RedisCache>,
    req: Request,
    next: Next,
) -> Result<Response> {
    let (key, ttl) = get_limit_params(&req, ip, &params);
    // Check if current error count exceeds limit
    if let Ok(count) = cache.get::<i64>(&key).await {
        if count > params.max {
            return Err(Error::TooManyRequests {
                limit: params.max,
                current: count,
            }
            .into());
        }
    }
    let resp = next.run(req).await;
    // Increment counter only on error responses
    if resp.status().as_u16() >= 400 {
        // Ignore Redis errors when incrementing
        let _ = cache.incr(&key, 1, Some(ttl)).await;
    }
    Ok(resp)
}

/// Standard rate limiting middleware
/// Increments counter for every request regardless of response status
pub async fn limiter(
    ClientIp(ip): ClientIp,
    State(params): State<LimitParams>,
    State(cache): State<&'static RedisCache>,
    req: Request,
    next: Next,
) -> Result<Response> {
    let (key, ttl) = get_limit_params(&req, ip, &params);

    // Increment counter and check against limit
    let count = cache.incr(&key, 1, Some(ttl)).await?;
    if count > params.max {
        return Err(Error::TooManyRequests {
            limit: params.max,
            current: count,
        }
        .into());
    }

    let resp = next.run(req).await;
    Ok(resp)
}
