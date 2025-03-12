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

type Result<T> = std::result::Result<T, Error>;

pub async fn processing_limit(
    State(state): State<&AppState>,
    req: Request,
    next: Next,
) -> Result<impl IntoResponse> {
    debug!(category = "middleware", "--> processing_limit");
    defer!(debug!(category = "middleware", "<-- processing_limit"););
    let limit = state.get_processing_limit();
    // limit < 0 means no limit
    if limit < 0 {
        let res = next.run(req).await;
        return Ok(res);
    }
    let count = state.inc_processing() + 1;
    if count > limit {
        state.dec_processing();
        return Err(new_exception_error_with_status(
            "Too many requests".to_string(),
            429,
        ));
    }
    let res = next.run(req).await;
    state.dec_processing();

    Ok(res)
}
