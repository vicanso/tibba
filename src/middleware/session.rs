use crate::config::{must_new_session_config, SessionConfig};
use crate::error::{HttpError, HttpResult};
use crate::state::get_app_state;
use crate::util;
use crate::{cache, task_local::*};
use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::header::{HeaderMap, HeaderValue};
use axum::http::request::Parts;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{async_trait, Json};
use axum_extra::extract::cookie::{Key, SignedCookieJar};
use cookie::CookieBuilder;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Validation};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::time::Duration;

static SESSION_CONFIG: Lazy<SessionConfig> = Lazy::new(must_new_session_config);
static SESSION_KEY: Lazy<Key> = Lazy::new(|| Key::from(SESSION_CONFIG.secret.as_bytes()));

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
    // 有效期
    exp: i64,
    // 创建时间
    iat: i64,
    id: String,
    account: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaimResp {
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
    fn get_key(&self) -> String {
        format!("ss:{}", self.id)
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
    pub fn refresh(&mut self) {
        self.exp = util::timestamp() + SESSION_CONFIG.ttl;
    }
    pub async fn save(&mut self) -> HttpResult<()> {
        if self.id.is_empty() {
            self.id = util::uuid();
        }
        cache::get_default_redis_cache()
            .set_struct(
                &self.get_key(),
                &self,
                Some(Duration::from_secs(SESSION_CONFIG.ttl as u64)),
            )
            .await?;
        Ok(())
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

// #[derive(Debug, Serialize)]
// pub struct AuthResp {
//     access_token: String,
//     token_type: String,
// }

// impl TryFrom<&Claim> for AuthResp {
//     type Error = HttpError;
//     fn try_from(value: &Claim) -> Result<Self, Self::Error> {
//         let access_token = encode(&jsonwebtoken::Header::default(), value, &KEYS.encoding)
//             .map_err(|_| {
//                 HttpError::new_with_category("Token creation error", JWT_ERROR_CATEGORY)
//             })?;
//         Ok(Self {
//             access_token,
//             token_type: "Bearer".to_string(),
//         })
//     }
// }

pub async fn get_claims_from_jar(jar: &SignedCookieJar) -> HttpResult<()> {
    if let Some(session_id) = jar.get(&SESSION_CONFIG.cookie) {
        println!("{session_id}");
    }

    // if let Some(session_id) = jar.get("session_id") {
    //     // fetch and render user...
    // } else {
    //     Err(StatusCode::UNAUTHORIZED)
    // }

    Ok(())
}

pub fn get_claims_from_headers(headers: &HeaderMap<HeaderValue>) -> HttpResult<Claim> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .unwrap_or_default();
    let is_missing = value.is_empty() || !value.starts_with("Bearer ");
    if is_missing {
        return Err(HttpError::new_with_category(
            "Missing credentials",
            JWT_ERROR_CATEGORY,
        ));
    }

    // let bearer = Authorization::<Bearer>::decode(value.replace("Bearer ", ""))
    //     .map_err(|err| HttpError::new_with_category(&err.to_string(), JWT_ERROR_CATEGORY))?;
    let result = decode::<Claim>(
        &value.replace("Bearer ", ""),
        &KEYS.decoding,
        &Validation::default(),
    )
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

pub async fn should_logged_in(mut req: Request<Body>, next: Next) -> HttpResult<Response> {
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
            util::set_account_to_context(req.extensions_mut(), util::Account::new(account.clone()));
            let mut resp = next.run(req).await;
            // 由于在session之前的中间件无法获取account的值
            // 因此又将account设置至resp extension中
            util::set_account_to_context(resp.extensions_mut(), util::Account::new(account));
            Ok(resp)
        })
        .await
}
