use super::JsonParams;
use crate::controller::JsonResult;
use crate::db::{add_user, find_user_by_account};
use crate::error::{HttpError, HttpResult};
use crate::middleware::{get_claims_from_headers, wait1s};
use crate::middleware::{load_session, AuthResp, Claim};
use crate::util;
use crate::{config, task_local::*, tl_error};
use axum::http::Request;
use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::Validate;

static APP_SECRET: Lazy<String> = Lazy::new(|| config::must_new_basic_config().secret);
#[derive(Debug, Clone, Serialize, Default)]
struct UserMeResp {
    name: String,
    expired_at: String,
    issued_at: String,
    time: String,
    roles: Option<Value>,
    groups: Option<Value>,
}

pub fn new_router() -> Router {
    let login_router = Router::new()
        .route("/login-token", get(login_token))
        .route("/register", post(register))
        // 登录设置为最少等待x秒，避免快速尝试
        .route("/login", post(login).layer(from_fn(wait1s)));
    let r = Router::new()
        .route("/me", get(me))
        .route("/refresh", get(refresh))
        .layer(from_fn(load_session));

    Router::new().nest("/users", r.merge(login_router))
}

async fn refresh(mut claims: Claim) -> JsonResult<AuthResp> {
    claims.refresh();
    let resp = (&claims).try_into()?;
    Ok(Json(resp))
}

async fn me<B>(mut jar: CookieJar, req: Request<B>) -> HttpResult<(CookieJar, Json<UserMeResp>)> {
    let mut account = "".to_string();
    let mut expired_at = "".to_string();
    let mut issued_at = "".to_string();
    match get_claims_from_headers(req.headers()) {
        Ok(claims) => {
            account = claims.get_account();
            expired_at = claims.get_expired_at();
            issued_at = claims.get_issued_at();
        }
        Err(err) => {
            tl_error!(err = err.message, "get claim fail");
        }
    }
    let mut roles = None;
    let mut groups = None;
    if !account.is_empty() {
        let result = find_user_by_account(&account).await?;
        if result.is_none() {
            return Err(HttpError::new("Account is not exists"));
        }
        let user = result.unwrap();
        roles = user.roles;
        groups = user.groups;
    }

    let me = UserMeResp {
        name: account,
        expired_at,
        issued_at,
        roles,
        groups,
        time: util::now(),
    };
    // 如果未设置device，则设置
    if util::get_device_id_from_cookie(&jar).is_empty() {
        jar = jar.add(util::generate_device_id_cookie());
    }

    Ok((jar, Json(me)))
}

fn generate_login_toke(timestamp: i64) -> String {
    let msg = format!("{}:{}", timestamp, APP_SECRET.as_str());
    util::sha256(msg.as_bytes())
}

#[derive(Deserialize, Validate)]
struct LoginParams {
    timestamp: i64,
    #[validate(length(min = 32))]
    token: String,
    #[validate(length(min = 2))]
    account: String,
    #[validate(length(min = 32))]
    password: String,
}

impl LoginParams {
    fn validate_token(&self) -> HttpResult<()> {
        // 测试环境需要，设置为0则跳过
        if self.timestamp == -1 && (util::is_development() || util::is_test()) {
            return Ok(());
        }
        if (self.timestamp - util::timestamp()).abs() > 60 {
            return Err(HttpError::new("Timestamp is invalid"));
        }
        if generate_login_toke(self.timestamp) != self.token {
            return Err(HttpError::new("Token is invalid"));
        }
        Ok(())
    }
}

async fn login(JsonParams(params): JsonParams<LoginParams>) -> JsonResult<AuthResp> {
    params.validate_token()?;

    let result = find_user_by_account(&params.account).await?;
    let account_password_err = HttpError::new("Account or password is wrong");
    if result.is_none() {
        return Err(account_password_err);
    }
    let password = result.unwrap().password;
    let msg = format!("{}:{password}", params.token);
    if util::sha256(msg.as_bytes()) != params.password {
        return Err(account_password_err);
    }
    let resp = (&Claim::new(&params.account)).try_into()?;
    Ok(Json(resp))
}

#[derive(Serialize)]
struct LoginTokenResp {
    timestamp: i64,
    token: String,
}
async fn login_token() -> JsonResult<LoginTokenResp> {
    let timestamp = util::timestamp();

    Ok(Json(LoginTokenResp {
        timestamp,
        token: generate_login_toke(timestamp),
    }))
}

#[derive(Deserialize, Validate)]
struct RegisterParams {
    #[validate(length(min = 2))]
    account: String,
    #[validate(length(min = 32))]
    password: String,
}
#[derive(Serialize)]
struct RegisterResp {
    id: i64,
    account: String,
}

async fn register(JsonParams(params): JsonParams<RegisterParams>) -> JsonResult<RegisterResp> {
    let result = add_user(&params.account, &params.password).await?;
    Ok(Json(RegisterResp {
        id: result.id,
        account: result.account,
    }))
}
