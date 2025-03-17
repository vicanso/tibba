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
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use tibba_error::{Error, new_error};
use tibba_middleware::Claim;
use tibba_util::{
    JsonParams, JsonResult, is_development, is_test, now, timestamp, timestamp_hash, uuid,
};
use tibba_util::{generate_device_id_cookie, get_device_id_from_cookie, validate_timestamp_hash};
use tibba_validator::*;
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
    fn validate_token(&self, secret: &str) -> Result<()> {
        // 测试环境需要，设置为0则跳过
        if self.ts <= 0 && (is_development() || is_test()) {
            return Ok(());
        }
        if (self.ts - timestamp()).abs() > 60 {
            return Err(new_error("Timestamp is invalid").into());
        }
        validate_timestamp_hash(self.ts, &self.token, &self.hash, secret)?;
        Ok(())
    }
}

async fn login(
    State(secret): State<String>,
    claim: Claim,
    JsonParams(params): JsonParams<LoginParams>,
) -> Result<Claim> {
    params.validate_token(&secret)?;

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

    let claim = claim.with_account(params.account);
    // let mut claim = Claim::new(&params.account);
    // // 记录session
    claim.save().await?;

    Ok(claim)
}

#[derive(Debug, Clone, Serialize, Default)]
struct UserMeResp {
    name: String,
    expired_at: String,
    issued_at: String,
    time: String,
}

async fn me(mut jar: CookieJar, claim: Claim) -> Result<(CookieJar, Json<UserMeResp>)> {
    let account = claim.get_account();
    // let mut roles = None;
    // let mut groups = None;
    // if !account.is_empty() {
    //     let result = find_user_by_account(&account).await?;
    //     if result.is_none() {
    //         return Err(HttpError::new("Account is not exists"));
    //     }
    //     let user = result.unwrap();
    //     roles = user.roles;
    //     groups = user.groups;
    // }

    // let me = UserMeResp {
    //     name: account,
    //     expired_at: claim.get_expired_at(),
    //     issued_at: claim.get_issued_at(),
    //     roles,
    //     groups,
    //     time: util::now(),
    // };
    // // 如果未设置device，则设置
    // if util::get_device_id_from_cookie(&jar).is_empty() {
    //     jar = jar.add(util::generate_device_id_cookie());
    // }
    if get_device_id_from_cookie(&jar).is_empty() {
        jar = jar.add(generate_device_id_cookie());
    }
    let info = UserMeResp {
        name: account,
        expired_at: claim.get_expired_at(),
        issued_at: claim.get_issued_at(),
        time: now(),
    };

    Ok((jar, Json(info)))
}

#[derive(Deserialize, Validate)]
struct RegisterParams {
    #[validate(custom(function = "x_user_account"))]
    account: String,
    #[validate(custom(function = "x_user_password"))]
    password: String,
}
#[derive(Serialize)]
struct RegisterResp {
    id: i64,
    account: String,
}

async fn register(JsonParams(params): JsonParams<RegisterParams>) -> JsonResult<RegisterResp> {
    println!("password: {}", params.password);
    // let result = add_user(&params.account, &params.password).await?;
    Ok(Json(RegisterResp {
        id: 123,
        account: params.account,
    }))
}

pub struct UserRouterParams {
    pub secret: String,
}

pub fn new_user_router(params: UserRouterParams) -> Router {
    Router::new()
        .route(
            "/login/token",
            get(login_token).with_state(params.secret.clone()),
        )
        .route("/login", post(login).with_state(params.secret))
        .route("/me", get(me))
        .route("/register", post(register))
}
