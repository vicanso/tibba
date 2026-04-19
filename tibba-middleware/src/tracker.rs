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

use super::LOG_TARGET;
use axum::extract::Request;
use axum::extract::State;
use axum::middleware::Next;
use axum::response::Response;
use tibba_error::Error;
use tibba_state::CTX;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy)]
pub struct TrackerParams {
    pub name: &'static str,
    pub step: &'static str,
}

impl From<(&'static str, &'static str)> for TrackerParams {
    fn from((name, step): (&'static str, &'static str)) -> Self {
        Self { name, step }
    }
}

/// Middleware that records user behavior events for audit and analytics.
///
/// Each event captures:
/// - Identity context: device_id, trace_id, account
/// - Business context: operation name and step label
/// - Outcome: HTTP status, success/failure result, elapsed time
/// - Failure detail: error message, category, sub-category, and whether it
///   was an infrastructure exception (vs. a normal business error)
pub async fn user_tracker(
    State(params): State<TrackerParams>,
    req: Request,
    next: Next,
) -> Result<Response> {
    let res = next.run(req).await;

    let ctx = CTX.get();
    // Milliseconds elapsed since the request entered the middleware stack
    let elapsed = ctx.elapsed_ms();
    let device_id = &ctx.device_id;
    let trace_id = &ctx.trace_id;
    // Authenticated account name; empty string when the user is not logged in
    let account = ctx.get_account();
    // HTTP status code — useful for correlating with access logs
    let status = res.status().as_u16();

    if status < 400 {
        info!(
            target: LOG_TARGET,
            device_id,
            trace_id,
            name = params.name,   // Logical operation name (e.g. "user_login")
            account = %account,
            step = params.step,   // Fine-grained step within the operation
            status,
            elapsed,
            result = "success",
            "user tracker",
        );
        return Ok(res);
    }

    // Extract structured error details from the response extensions.
    // If no Error is attached (e.g. the handler panicked), treat as an
    // infrastructure exception so on-call alerts fire correctly.
    let (error, error_category, error_sub_category, error_exception) = res
        .extensions()
        .get::<Error>()
        .map(|err| {
            (
                Some(err.message.clone()),
                Some(err.category.clone()),
                err.sub_category.clone(),
                // true when the error originated from infrastructure
                // (network timeout, downstream failure, etc.)
                err.exception.unwrap_or_default(),
            )
        })
        .unwrap_or((None, None, None, true));

    error!(
        target: LOG_TARGET,
        device_id,
        trace_id,
        name = params.name,
        account = %account,
        step = params.step,
        status,
        error,
        error_category,
        error_sub_category,
        // Distinguishes infrastructure exceptions from normal business errors
        error_exception,
        elapsed,
        result = "failure",
        "user tracker",
    );
    Ok(res)
}
