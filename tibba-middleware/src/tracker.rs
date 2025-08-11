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
use tibba_error::{Error, new_error};
use tibba_state::CTX;
use tibba_util::read_http_body;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;

async fn get_http_error(res: Response) -> Result<(Response, Error)> {
    let (parts, body) = res.into_parts();
    let data = read_http_body(body)
        .await
        .map_err(|e| new_error(e.to_string()))?;
    let err = serde_json::from_slice::<Error>(&data).map_err(|e| new_error(e.to_string()))?;
    Ok((Response::from_parts(parts, Body::from(data)), err))
}

pub async fn user_tracker(
    State((name, step)): State<(String, String)>,
    req: Request,
    next: Next,
) -> Result<Response> {
    let category = "tracker";
    let res = next.run(req).await;
    let ctx = CTX.get();
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
            result = "success",
        );
        return Ok(res);
    }
    let data = get_http_error(res)
        .await
        .map_err(|e| e.with_category(category))?;
    let err = data.1;
    // TODO add tracker
    error!(
        category = category,
        device_id,
        trace_id,
        name = name,
        account,
        step,
        error = err.message,
        error_category = err.category,
        error_sub_category = err.sub_category,
        error_code = err.code,
        error_exception = err.exception,
        result = "failure",
    );
    Ok(data.0)
}
