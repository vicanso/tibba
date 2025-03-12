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
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use scopeguard::defer;
use std::sync::Arc;
use tibba_state::{CTX, Context};
use tibba_util::{
    get_device_id_from_cookie, set_header_if_not_exist, set_no_cache_if_not_exist, uuid,
};
use tracing::debug;

pub async fn entry(jar: CookieJar, req: Request, next: Next) -> Response {
    debug!(category = "middleware", "--> entry");
    defer!(debug!(category = "middleware", "<-- entry"););
    let device_id = get_device_id_from_cookie(&jar);
    let trace_id = uuid();
    let ctx = Context::new(&device_id, &trace_id);
    let mut res = CTX
        .scope(Arc::new(ctx), async { next.run(req).await })
        .await;
    let headers = res.headers_mut();
    set_no_cache_if_not_exist(headers);
    let _ = set_header_if_not_exist(headers, "X-Trace-Id", &trace_id);

    res
}
