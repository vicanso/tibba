use crate::cache::get_default_redis_cache;
use crate::error::{HttpError, HttpResult};
use crate::state::AppState;
use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};
use axum_client_ip::InsecureClientIp;
use std::net::IpAddr;
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

#[derive(Debug, Clone, Default)]
pub enum LimitType {
    #[default]
    Ip,
}

#[derive(Debug, Clone, Default)]
pub struct LimitParams {
    pub limit_type: LimitType,
    pub category: String,
    pub max: i64,
    pub ttl: Duration,
}

impl LimitParams {
    pub fn new(max: i64, secs: u64, category: &str) -> Self {
        LimitParams {
            limit_type: LimitType::Ip,
            category: category.to_string(),
            max,
            ttl: Duration::from_secs(secs),
            ..Default::default()
        }
    }
}

fn get_limit_params(ip: IpAddr, params: &LimitParams) -> (String, Duration) {
    let mut key = match params.limit_type {
        _ => ip.to_string(),
    };
    if !params.category.is_empty() {
        key = format!("{}:{key}", params.category);
    }
    let mut ttl = params.ttl;
    if ttl.is_zero() {
        ttl = Duration::from_secs(5 * 60);
    }
    (key, ttl)
}

pub async fn error_limiter(
    InsecureClientIp(ip): InsecureClientIp,
    State(params): State<LimitParams>,
    req: Request<Body>,
    next: Next,
) -> HttpResult<Response<Body>> {
    let (key, ttl) = get_limit_params(ip, &params);
    // 获取失败的忽略
    if let Ok(count) = get_default_redis_cache().get_value::<i64>(&key).await {
        if count > params.max {
            let msg = format!("请求过于频繁，请稍候再试！({count}/{})", params.max);
            return Err(HttpError::new_with_category(&msg, "error_limiter"));
        }
    }
    let resp = next.run(req).await;
    // 如果失败了则+1
    if resp.status().as_u16() >= 400 {
        // 设置失败也忽略
        let _ = get_default_redis_cache().incr(&key, 1, Some(ttl)).await;
    }
    Ok(resp)
}

pub async fn limiter(
    InsecureClientIp(ip): InsecureClientIp,
    State(params): State<LimitParams>,
    req: Request<Body>,
    next: Next,
) -> HttpResult<Response<Body>> {
    let (key, ttl) = get_limit_params(ip, &params);

    let count = get_default_redis_cache().incr(&key, 1, Some(ttl)).await?;
    if count > params.max {
        let msg = format!("请求过于频繁，请稍候再试！({count}/{})", params.max);
        return Err(HttpError::new_with_category(&msg, "limiter"));
    }
    let resp = next.run(req).await;
    Ok(resp)
}
