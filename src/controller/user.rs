use crate::cache::get_default_redis_cache;
use crate::controller::JsonResult;
use crate::error::HttpResult;
use crate::middleware::{get_claims_from_headers, wait1s};
use crate::middleware::{load_session, AuthResp, Claims};
use crate::util::{generate_device_id_cookie, get_device_id_from_cookie, now};
use crate::{task_local::*, tl_error};
use axum::http::Request;
use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Default)]
struct UserMeResp {
    name: String,
    expired_at: String,
    issued_at: String,
    time: String,
}

pub fn new_router() -> Router {
    let login_router = Router::new()
        .route("/v1/login", post(login))
        // 登录设置为最少等待x秒，避免快速尝试
        .layer(from_fn(wait1s));
    let me_router = Router::new()
        .route("/v1/me", get(me))
        .route("/v1/refresh", get(refresh));
    let r = Router::new()
        .route("/v1/login-times", get(get_login_times))
        .layer(from_fn(load_session));

    Router::new().nest("/users", r.merge(login_router).merge(me_router))
}

async fn refresh(mut claims: Claims) -> JsonResult<AuthResp> {
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

    let me = UserMeResp {
        name: account,
        expired_at,
        issued_at,
        time: now(),
    };
    // 如果未设置device，则设置
    if get_device_id_from_cookie(&jar).is_empty() {
        jar = jar.add(generate_device_id_cookie());
    }

    Ok((jar, Json(me)))
}

#[derive(Deserialize)]
struct LoginParams {
    account: String,
}

async fn login(Json(params): Json<LoginParams>) -> JsonResult<AuthResp> {
    // TODO 账号校验
    let resp = (&Claims::new(&params.account)).try_into()?;
    Ok(Json(resp))
}

#[derive(Debug, Clone, Serialize, Default)]
struct LoginTimesResp {
    pub count: i64,
}
async fn get_login_times(jar: CookieJar) -> JsonResult<LoginTimesResp> {
    let device_id = get_device_id_from_cookie(&jar);
    let cache = get_default_redis_cache();
    // 如果未设置device，则设置
    let mut count: i64 = 0;
    if !device_id.is_empty() {
        count = cache
            .incr(&device_id, 1, Some(Duration::from_secs(60 * 60)))
            .await?;
    }

    Ok(Json(LoginTimesResp { count }))
}
