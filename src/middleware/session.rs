use crate::config::{must_new_session_config, SessionConfig};
use crate::db::find_user_by_account;
use crate::error::{HttpError, HttpResult};
use crate::util;
use crate::{cache, task_local::*};
use axum::body::Body;
use axum::extract::{FromRequestParts, State};
use axum::http::header::{HeaderMap, HeaderValue};
use axum::http::request::Parts;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{async_trait, Json};
use axum_extra::extract::cookie::{Key, SignedCookieJar};
use cookie::CookieBuilder;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::time::Duration;

static SESSION_CONFIG: Lazy<SessionConfig> = Lazy::new(must_new_session_config);
static SESSION_KEY: Lazy<Key> = Lazy::new(|| Key::from(SESSION_CONFIG.secret.as_bytes()));

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Claim {
    // 有效期
    exp: i64,
    // 创建时间
    iat: i64,
    id: String,
    account: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ClaimResp {
    account: String,
}

impl Claim {
    pub fn new(account: &str) -> Self {
        let iat = util::timestamp();
        Claim {
            exp: iat + SESSION_CONFIG.ttl,
            iat,
            id: "".to_string(),
            account: account.to_string(),
        }
    }
    pub async fn new_from_redis(id: &str) -> HttpResult<Self> {
        let key = Self::get_key(id);
        let result = cache::get_default_redis_cache().get_struct(&key).await?;
        Ok(result.unwrap_or_default())
    }
    fn get_key(id: &str) -> String {
        format!("ss:{id}")
    }
    pub fn get_account(&self) -> String {
        self.account.clone()
    }
    pub fn get_expired_at(&self) -> String {
        util::from_timestamp(self.exp, 0)
    }
    pub fn get_issued_at(&self) -> String {
        util::from_timestamp(self.iat, 0)
    }
    pub fn is_expired(&self) -> bool {
        let value = util::timestamp();
        // 如果创建时间已超过30天，则认为过期
        if value - self.iat > 30 * 24 * 3600 {
            return true;
        }
        // 已过期
        if self.exp < value {
            return true;
        }

        false
    }
    pub async fn refresh(&mut self) -> HttpResult<()> {
        self.exp = util::timestamp() + SESSION_CONFIG.ttl;
        self.save().await
    }
    pub async fn save(&mut self) -> HttpResult<()> {
        if self.id.is_empty() {
            self.id = util::uuid();
        }
        cache::get_default_redis_cache()
            .set_struct(
                &Self::get_key(&self.id),
                &self,
                Some(Duration::from_secs(SESSION_CONFIG.ttl as u64)),
            )
            .await?;
        Ok(())
    }
    pub fn destroy(&mut self) {
        self.id = "".to_string();
    }
}

impl IntoResponse for Claim {
    fn into_response(self) -> Response {
        let c = CookieBuilder::new(&SESSION_CONFIG.cookie, self.id.clone())
            .path("/")
            .http_only(true)
            .max_age(time::Duration::seconds(SESSION_CONFIG.ttl));
        let jar = SignedCookieJar::new(SESSION_KEY.clone());

        (
            jar.add(c),
            Json(ClaimResp {
                account: self.account,
            }),
        )
            .into_response()
    }
}

async fn get_claim_from_headers(headers: &HeaderMap<HeaderValue>) -> HttpResult<Claim> {
    let jar = SignedCookieJar::from_headers(headers, SESSION_KEY.clone());
    let result = if let Some(session_id) = jar.get(&SESSION_CONFIG.cookie) {
        let claim = Claim::new_from_redis(session_id.value()).await?;
        // 如果已过期
        if claim.is_expired() {
            Claim::default()
        } else {
            claim
        }
    } else {
        Claim::default()
    };
    Ok(result)
}

#[async_trait]
impl<S> FromRequestParts<S> for Claim
where
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(claim) = parts.extensions.get::<Claim>() {
            return Ok(claim.clone());
        }
        let claim = get_claim_from_headers(&parts.headers).await?;
        parts.extensions.insert(claim.clone());
        Ok(claim)
    }
}

async fn load_claim(
    should_logged_in: bool,
    mut req: Request<Body>,
    next: Next,
) -> HttpResult<Response> {
    let claim = get_claim_from_headers(req.headers()).await?;
    if should_logged_in && claim.account.is_empty() {
        return Err(HttpError {
            message: "Should be login first".to_string(),
            status: StatusCode::UNAUTHORIZED.as_u16(),
            ..Default::default()
        });
    }
    req.extensions_mut().insert(claim.clone());
    let account = claim.get_account();

    ACCOUNT
        .scope(account.clone(), async {
            util::set_account_to_context(req.extensions_mut(), util::Account::new(account.clone()));
            let mut resp = next.run(req).await;
            // 由于在session之前的中间件无法获取account的值
            // 因此又将account设置至resp extension中
            util::set_account_to_context(resp.extensions_mut(), util::Account::new(account));
            Ok(resp)
        })
        .await
}

pub async fn load_session(req: Request<Body>, next: Next) -> HttpResult<Response> {
    load_claim(false, req, next).await
}

pub async fn should_logged_in(req: Request<Body>, next: Next) -> HttpResult<Response> {
    load_claim(true, req, next).await
}

pub async fn validate_roles(
    State(valid_roles): State<Vec<String>>,
    req: Request<Body>,
    next: Next,
) -> HttpResult<Response> {
    let claim = get_claim_from_headers(req.headers()).await?;
    if claim.account.is_empty() {
        return Err(HttpError {
            message: "Should be login first".to_string(),
            status: StatusCode::UNAUTHORIZED.as_u16(),
            ..Default::default()
        });
    }
    // 因为已登录成功，因此账号不存在不会发生
    let result = find_user_by_account(&claim.account)
        .await?
        .ok_or(HttpError::new("账号不存在"))?;
    let mut valid = false;
    if let Some(roles) = result.roles {
        let roles = util::json_value_to_strings(&roles)?.unwrap_or_default();
        roles.iter().for_each(|item| {
            if !valid && valid_roles.contains(item) {
                valid = true;
            }
        });
    }
    if !valid {
        return Err(HttpError {
            message: "当前登录账号权限不满足".to_string(),
            status: StatusCode::FORBIDDEN.as_u16(),
            ..Default::default()
        });
    }
    let resp = next.run(req).await;
    Ok(resp)
}
