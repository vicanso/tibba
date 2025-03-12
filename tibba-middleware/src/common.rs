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
use tibba_state::CTX;
use tokio::time::sleep;
use tracing::debug;

#[derive(Debug, Clone, Default)]
pub struct WaitParams {
    pub ms: u64,
    pub only_error_occurred: bool,
}

impl WaitParams {
    pub fn new(ms: u64) -> Self {
        Self {
            ms,
            ..Default::default()
        }
    }
}

pub async fn wait(State(params): State<WaitParams>, req: Request, next: Next) -> Response {
    debug!(category = "middleware", "--> wait");
    defer!(debug!(category = "middleware", "<-- wait"););
    let res = next.run(req).await;
    // only wait if error occurred
    if params.only_error_occurred && res.status().as_u16() < 400 {
        return res;
    }
    let elapsed = CTX.get().elapsed().as_millis();
    let offset = params.ms - elapsed as u64;
    // if offset is less than 10, don't wait
    if offset >= 10 {
        sleep(Duration::from_millis(offset)).await
    }
    res
}
