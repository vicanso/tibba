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

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tibba_util::{JsonResult, timestamp_hash, uuid};

#[derive(Serialize)]
struct LoginTokenResp {
    ts: i64,
    hash: String,
    token: String,
}
async fn login_token(State(secret): State<String>) -> JsonResult<LoginTokenResp> {
    let token = uuid();
    let (ts, hash) = timestamp_hash(&token, &secret);

    Ok(Json(LoginTokenResp { ts, hash, token }))
}

pub struct UserRouterParams {
    pub secret: String,
}

pub fn new_user_router(params: UserRouterParams) -> Router {
    Router::new()
        .route("/login/token", get(login_token))
        .with_state(params.secret)
}
