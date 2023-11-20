use crate::config::{must_new_session_config, SessionConfig};
use crate::error::{HttpError, HttpResult};
use crate::task_local::*;
use crate::util::{from_timestamp, random_string, set_account_to_context, Account};
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::headers::authorization::Bearer;
use axum::headers::{Authorization, Header};
use axum::http::header::{HeaderMap, HeaderValue};
use axum::http::request::Parts;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Validation};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

static SESSION_CONFIG: Lazy<SessionConfig> = Lazy::new(must_new_session_config);

static KEYS: Lazy<Keys> = Lazy::new(|| Keys::new(SESSION_CONFIG.secret.as_bytes()));

static JWT_ERROR_CATEGORY: &str = "jwt";

struct Keys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl Keys {
    fn new(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claim {
    // Required (validate_exp defaults to true in validation). Expiration time (as UTC timestamp)
    exp: usize,
    // Optional. Issued at (as UTC timestamp)
    iat: usize,
    id: String,
    account: String,
}

impl Claim {
    pub fn new(account: &str) -> Self {
        let iat = now();
        Claim {
            exp: iat + SESSION_CONFIG.ttl,
            iat,
            id: random_string(8),
            account: account.to_string(),
        }
    }
    pub fn get_account(&self) -> String {
        self.account.clone()
    }
    pub fn get_expired_at(&self) -> String {
        from_timestamp(self.exp as i64, 0)
    }
    pub fn get_issued_at(&self) -> String {
        from_timestamp(self.iat as i64, 0)
    }
    pub fn is_expired(&self) -> bool {
        let value = now();
        // 已过期
        if self.exp < value {
            return false;
        }
        // 如果创建时间已超过30天，则认为过期
        if value - self.iat > 30 * 24 * 3600 {
            return true;
        }
        false
    }
    pub fn refresh(&mut self) {
        self.exp = now() + SESSION_CONFIG.ttl;
    }
}

#[derive(Debug, Serialize)]
pub struct AuthResp {
    access_token: String,
    token_type: String,
}

impl TryFrom<&Claim> for AuthResp {
    type Error = HttpError;
    fn try_from(value: &Claim) -> Result<Self, Self::Error> {
        let access_token = encode(&jsonwebtoken::Header::default(), value, &KEYS.encoding)
            .map_err(|_| {
                HttpError::new_with_category("Token creation error", JWT_ERROR_CATEGORY)
            })?;
        Ok(Self {
            access_token,
            token_type: "Bearer".to_string(),
        })
    }
}
fn now() -> usize {
    chrono::Utc::now().timestamp() as usize
}

pub fn get_claims_from_headers(headers: &HeaderMap<HeaderValue>) -> HttpResult<Claim> {
    let mut values = headers.get_all(Authorization::<Bearer>::name()).iter();
    let is_missing = values.size_hint() == (0, Some(0));
    if is_missing {
        return Err(HttpError::new_with_category(
            "Missing credentials",
            JWT_ERROR_CATEGORY,
        ));
    }

    let bearer = Authorization::<Bearer>::decode(&mut values)
        .map_err(|err| HttpError::new_with_category(&err.to_string(), JWT_ERROR_CATEGORY))?;
    let result = decode::<Claim>(bearer.token(), &KEYS.decoding, &Validation::default())
        .map_err(|_| HttpError::new_with_category("Invalid token", JWT_ERROR_CATEGORY))?;
    let claims = result.claims;
    if claims.is_expired() {
        return Err(HttpError::new_with_category(
            "Claim is expired",
            JWT_ERROR_CATEGORY,
        ));
    }
    Ok(claims)
}

#[async_trait]
impl<S> FromRequestParts<S> for Claim
where
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        get_claims_from_headers(&parts.headers)
    }
}

pub async fn load_session<B>(mut req: Request<B>, next: Next<B>) -> HttpResult<Response> {
    let claims = get_claims_from_headers(req.headers())?;
    if claims.account.is_empty() {
        return Err(HttpError {
            message: "Should be login first".to_string(),
            status: StatusCode::UNAUTHORIZED.as_u16(),
            ..Default::default()
        });
    }
    let account = claims.get_account();

    ACCOUNT
        .scope(account.clone(), async {
            set_account_to_context(req.extensions_mut(), Account::new(account.clone()));
            let mut resp = next.run(req).await;
            // 由于在session之前的中间件无法获取account的值
            // 因此又将account设置至resp extension中
            set_account_to_context(resp.extensions_mut(), Account::new(account));
            Ok(resp)
        })
        .await
}
