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
use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use tibba_cache::RedisCache;
use tibba_error::{Error, new_error};
use tibba_middleware::{Session, UserSession, validate_captcha};
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
    session: Session,
    JsonParams(params): JsonParams<LoginParams>,
) -> Result<Session> {
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

    let session = session.with_account(params.account);
    session.save().await?;

    Ok(session)
}

#[derive(Debug, Clone, Serialize, Default)]
struct UserMeResp {
    account: String,
    expired_at: String,
    issued_at: String,
    time: String,
    can_renew: bool,
    email: Option<String>,
    avatar: Option<String>,
}

async fn me(
    State(pool): State<&'static MySqlPool>,
    mut jar: CookieJar,
    session: Session,
) -> Result<(CookieJar, Json<UserMeResp>)> {
    let account = session.get_account();
    if get_device_id_from_cookie(&jar).is_empty() {
        jar = jar.add(generate_device_id_cookie());
    }
    if !session.is_login() {
        return Ok((jar, Json(UserMeResp::default())));
    }
    let user = ModelUser::get_by_account(pool, &account)
        .await?
        .ok_or(new_error("User not found"))?;
    let info = UserMeResp {
        account,
        expired_at: session.get_expired_at(),
        issued_at: session.get_issued_at(),
        time: now(),
        can_renew: session.can_renew(),
        email: user.email,
        avatar: user.avatar,
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

async fn refresh_session(mut session: UserSession) -> Result<Session> {
    if !session.can_renew() {
        return Ok(session.into());
    }
    session.refresh();
    session.save().await?;
    Ok(session.into())
}

async fn logout(mut session: Session) -> Session {
    session.reset();
    session
}

pub struct UserRouterParams {
    pub secret: String,
    pub magic_code: String,
    pub pool: &'static MySqlPool,
    pub cache: &'static RedisCache,
}

pub fn new_user_router(params: UserRouterParams) -> Router {
    Router::new()
        .route(
            "/login/token",
            get(login_token).with_state(params.secret.clone()),
        )
        .route(
            "/login",
            post(login)
                .with_state((params.secret, params.pool))
                .layer(from_fn_with_state(
                    (params.magic_code, params.cache),
                    validate_captcha,
                )),
        )
        .route("/me", get(me).with_state(params.pool))
        .route("/refresh", patch(refresh_session))
        .route("/register", post(register).with_state(params.pool))
        .route("/logout", delete(logout))
}
