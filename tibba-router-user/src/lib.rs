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
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tibba_error::{Error, new_error};
use tibba_middleware::Claim;
use tibba_util::{
    JsonParams, JsonResult, is_development, is_test, timestamp, timestamp_hash, uuid,
};
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

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

#[derive(Deserialize, Validate, Debug)]
struct LoginParams {
    ts: i64,
    #[validate(length(min = 32))]
    token: String,
    #[validate(length(min = 32))]
    hash: String,
    #[validate(length(min = 2))]
    account: String,
    #[validate(length(min = 32))]
    password: String,
}

impl LoginParams {
    fn validate_token(&self) -> Result<()> {
        // 测试环境需要，设置为0则跳过
        if self.ts <= 0 && (is_development() || is_test()) {
            return Ok(());
        }
        if (self.ts - timestamp()).abs() > 60 {
            return Err(new_error("Timestamp is invalid").into());
        }
        // validate_timestamp_hash(self.ts, &self.token, &self.hash)?;
        Ok(())
    }
}

async fn login(claim: Claim, JsonParams(params): JsonParams<LoginParams>) -> JsonResult<Claim> {
    params.validate_token()?;
    println!("claim: {claim:?}");
    println!("{params:?}");

    // params.validate_token()?;

    // let result = find_user_by_account(&params.account).await?;
    // let account_password_err = HttpError::new("Account or password is wrong");
    // if result.is_none() {
    //     return Err(account_password_err);
    // }
    // let password = result.unwrap().password;
    // let msg = format!("{}:{password}", params.hash);
    // if util::sha256(msg.as_bytes()) != params.password {
    //     return Err(account_password_err);
    // }

    // let mut claim = Claim::new(&params.account);
    // // 记录session
    // claim.save().await?;

    Ok(Json(claim.with_account(params.account)))
}

pub struct UserRouterParams {
    pub secret: String,
}

pub fn new_user_router(params: UserRouterParams) -> Router {
    Router::new()
        .route("/login/token", get(login_token).with_state(params.secret))
        .route("/login", post(login))
}
