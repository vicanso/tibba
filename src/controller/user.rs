use super::JsonParams;
use crate::controller::JsonResult;
use crate::db::{add_user, find_user_by_account};
use crate::error::{HttpError, HttpResult};
use crate::middleware::{get_claims_from_headers, get_claims_from_jar, ip_login_limit, wait1s};
use crate::middleware::{should_logged_in, Claim};
use crate::util;
use crate::{task_local::*, tl_error};
use axum::http::Request;
use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use axum_extra::extract::cookie::SignedCookieJar;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::Validate;

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
        .route(
            "/login",
            post(login)
                .layer(from_fn(wait1s))
                .layer(from_fn(ip_login_limit)),
        );
    let r = Router::new()
        .route("/me", get(me))
        // .route("/refresh", get(refresh))
        .layer(from_fn(should_logged_in));

    Router::new().nest("/users", r.merge(login_router))
}

// async fn refresh(mut claims: Claim) -> JsonResult<AuthResp> {
//     claims.refresh();
//     let resp: AuthResp = (&claims).try_into()?;
//     Ok(resp.into())
// }

async fn me<B>(
    mut jar: CookieJar,
    // signed_jar: SignedCookieJar,
    req: Request<B>,
) -> HttpResult<(CookieJar, Json<UserMeResp>)> {
    // get_claims_from_jar(&signed_jar).await?;

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

    Ok((jar, me.into()))
}

#[derive(Deserialize, Validate)]
struct LoginParams {
    timestamp: i64,
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
    fn validate_token(&self) -> HttpResult<()> {
        // 测试环境需要，设置为0则跳过
        if self.timestamp <= 0 && (util::is_development() || util::is_test()) {
            return Ok(());
        }
        if (self.timestamp - util::timestamp()).abs() > 60 {
            return Err(HttpError::new("Timestamp is invalid"));
        }
        util::validate_sign_hash(&self.token, &self.hash)?;
        Ok(())
    }
}

async fn login(JsonParams(params): JsonParams<LoginParams>) -> HttpResult<Claim> {
    params.validate_token()?;

    let result = find_user_by_account(&params.account).await?;
    let account_password_err = HttpError::new("Account or password is wrong");
    if result.is_none() {
        return Err(account_password_err);
    }
    let password = result.unwrap().password;
    let msg = format!("{}:{password}", params.hash);
    if util::sha256(msg.as_bytes()) != params.password {
        return Err(account_password_err);
    }

    let mut claim = Claim::new(&params.account);
    // 记录session
    claim.save().await?;

    Ok(claim)
}

#[derive(Serialize)]
struct LoginTokenResp {
    ts: i64,
    hash: String,
    token: String,
}
async fn login_token() -> JsonResult<LoginTokenResp> {
    let token = util::uuid();
    let (ts, hash) = util::timestamp_hash(&token);

    Ok(Json(LoginTokenResp { ts, hash, token }))
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
