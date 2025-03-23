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
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use tibba_error::{Error, new_error};
use tibba_middleware::Claim;
use tibba_model::ModelUser;
use tibba_util::{
    JsonParams, JsonResult, is_development, is_test, now, sha256, timestamp, timestamp_hash, uuid,
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
        // only for test
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
    State((secret, pool)): State<(String, &'static MySqlPool)>,
    claim: Claim,
    JsonParams(params): JsonParams<LoginParams>,
) -> Result<Claim> {
    params.validate_token(&secret)?;
    let account_password_err = new_error("Account or password is wrong");
    let Some(user) = ModelUser::get_by_account(pool, &params.account).await? else {
        return Err(account_password_err.into());
    };

    let password = user.password;
    let msg = format!("{}:{password}", params.hash);
    if sha256(msg.as_bytes()) != params.password {
        return Err(account_password_err.into());
    }

    let claim = claim.with_account(params.account);
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

async fn register(
    State(pool): State<&'static MySqlPool>,
    JsonParams(params): JsonParams<RegisterParams>,
) -> JsonResult<RegisterResp> {
    let id = ModelUser::insert(pool, &params.account, &params.password).await?;
    Ok(Json(RegisterResp {
        id,
        account: params.account,
    }))
}

async fn refresh_session(claim: Claim) -> Result<Claim> {
    if !claim.can_renew() {
        return Ok(claim);
    }
    let account = claim.get_account();
    let claim = claim.with_account(account);
    claim.save().await?;
    Ok(claim)
}

pub struct UserRouterParams {
    pub secret: String,
    pub pool: &'static MySqlPool,
}

pub fn new_user_router(params: UserRouterParams) -> Router {
    Router::new()
        .route(
            "/login/token",
            get(login_token).with_state(params.secret.clone()),
        )
        .route(
            "/login",
            post(login).with_state((params.secret, params.pool)),
        )
        .route("/me", get(me))
        .route("/refresh", patch(refresh_session))
        .route("/register", post(register).with_state(params.pool))
}
