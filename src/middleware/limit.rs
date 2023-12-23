use crate::cache::get_default_redis_cache;
use crate::error::{HttpError, HttpResult};
use crate::state::AppState;
use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};
use axum_client_ip::InsecureClientIp;
use std::time::Duration;

pub async fn processing_limit(
    State(state): State<&AppState>,
    req: Request<Body>,
    next: Next,
) -> HttpResult<Response> {
    if state.increase_processing() > state.processing_limit && state.processing_limit != 0 {
        state.decrease_processing();
        return Err(HttpError::new_with_status("Too Many Requests", 429));
    }
    let resp = next.run(req).await;
    state.decrease_processing();
    Ok(resp)
}

pub struct LimitParams {
    key: String,
    max: i64,
    ttl: Duration,
}

async fn limiter(
    params: LimitParams,
    req: Request<Body>,
    next: Next,
) -> HttpResult<Response<Body>> {
    let count = get_default_redis_cache()
        .incr(&params.key, 1, Some(params.ttl))
        .await?;
    if count > params.max {
        let msg = format!("请求过于频繁，请稍候再试！({count}/{})", params.max);
        return Err(HttpError::new_with_category(&msg, "limiter"));
    }
    let resp = next.run(req).await;
    Ok(resp)
}

pub async fn ip_login_limit(
    InsecureClientIp(ip): InsecureClientIp,
    req: Request<Body>,
    next: Next,
) -> HttpResult<Response<Body>> {
    let key = format!("ip-login:{}", ip);
    limiter(
        LimitParams {
            key,
            max: 100,
            // 24小时
            ttl: Duration::from_secs(24 * 3600),
        },
        req,
        next,
    )
    .await
}
