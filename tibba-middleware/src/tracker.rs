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
use tibba_error::Error;
use tibba_state::CTX;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;

pub async fn user_tracker(
    State((name, step)): State<(&str, &str)>,
    req: Request,
    next: Next,
) -> Result<Response> {
    let category = "tracker";
    let res = next.run(req).await;
    let ctx = CTX.get();
    let elapsed = ctx.elapsed().as_millis();
    let device_id = &ctx.device_id;
    let trace_id = &ctx.trace_id;
    let account = ctx.get_account();
    if res.status().as_u16() < 400 {
        info!(
            category,
            device_id,
            trace_id,
            name,
            account,
            step = step,
            elapsed,
            result = "success",
        );
        return Ok(res);
    }
    let mut error = None;
    let mut error_category = None;
    let mut error_sub_category = None;
    // it should get error success, otherwise it should be exception error
    let mut error_exception = true;
    if let Some(err) = res.extensions().get::<Error>() {
        error = Some(err.message.clone());
        error_category = Some(err.category.clone());
        error_sub_category = err.sub_category.clone();
        error_exception = err.exception.unwrap_or_default();
    }
    // TODO add tracker
    error!(
        category = category,
        device_id,
        trace_id,
        name = name,
        account,
        step,
        error,
        error_category,
        error_sub_category,
        error_exception,
        elapsed,
        result = "failure",
    );
    Ok(res)
}
