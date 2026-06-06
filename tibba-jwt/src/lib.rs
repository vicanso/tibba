// Copyright 2026 Tree xie.
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

//! tibba-jwt
//!
//! HS256 JWT 鉴权路径，与现有 Session+Cookie 路径正交：
//!
//! - **access token** = HS256 JWT，claims 含 sub / account / roles / permissions / exp / jti
//!   完全无状态，验签即放行，**不查 Redis**
//! - **refresh token** 是 opaque UUID 字符串，**不是 JWT**，存 Redis
//!   `jwt_refresh:{token} → (user_id, account)`，便于 logout / 强制下线 `cache.del()`
//!
//! ## 集成
//!
//! ```ignore
//! // 1. 启动期（src/main.rs 在 config 加载后）
//! let cfg = must_get_jwt_config();
//! if cfg.is_configured() {
//!     let signer = tibba_jwt::JwtSigner::from_config(cfg)?;
//!     tibba_jwt::init_global_signer(signer).expect("set global jwt signer");
//! }
//!
//! // 2. handler 内签发
//! let signer = tibba_jwt::must_global_signer();
//! let access = signer.sign_access(user.id, &user.account, roles, perms)?;
//!
//! // 3. handler 内取已鉴权用户（自动 401 / 403）
//! async fn me(user: JwtUser) -> ... { user.user_id ... }
//! ```

use once_cell::sync::OnceCell;
use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use std::time::Duration;
use tibba_error::Error as BaseError;

pub use claims::{Claims, JwtUser};

mod claims;

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:jwt=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:jwt";

/// 全局唯一 JwtSigner，由 binary 在启动期初始化。
static GLOBAL_SIGNER: OnceCell<JwtSigner> = OnceCell::new();

/// tibba-jwt 模块对外的错误类型。
#[derive(Debug, Snafu)]
pub enum Error {
    /// 配置 secret 为空 / TTL 非法等，`from_config` / 签名前置校验
    #[snafu(display("jwt secret not configured"))]
    NotConfigured,
    /// JWT 编码（签名）失败
    #[snafu(display("jwt sign failed: {source}"))]
    Sign {
        #[snafu(source(from(jsonwebtoken::errors::Error, Box::new)))]
        source: Box<jsonwebtoken::errors::Error>,
    },
    /// JWT 解码（验签 / 过期 / claims 校验）失败
    #[snafu(display("jwt verify failed: {source}"))]
    Verify {
        #[snafu(source(from(jsonwebtoken::errors::Error, Box::new)))]
        source: Box<jsonwebtoken::errors::Error>,
    },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::NotConfigured => BaseError::new("jwt secret not configured")
                .with_sub_category("not_configured")
                .with_status(503)
                .with_exception(true),
            Error::Sign { source } => BaseError::new(source)
                .with_sub_category("sign")
                .with_status(500)
                .with_exception(true),
            // 验签失败统一映射 401——可能是过期 / 签名错 / 篡改
            Error::Verify { source } => BaseError::new(source)
                .with_sub_category("verify")
                .with_status(401)
                .with_exception(false),
        };
        err.with_category("jwt")
    }
}

fn default_access_ttl() -> Duration {
    Duration::from_secs(15 * 60) // 15 minutes
}

fn default_refresh_ttl() -> Duration {
    Duration::from_secs(7 * 24 * 60 * 60) // 7 days
}

fn default_issuer() -> String {
    "tibba".to_string()
}

/// 应用配置中的 `[jwt]` 段。
///
/// 字段：
/// - `secret`：HS256 共享密钥，**必填**且**绝不入仓**——推荐 env var
///   `TIBBA_WEB__JWT__SECRET` 注入。空时 `is_configured()` 返回 false，
///   binary 不会初始化 GLOBAL_SIGNER，相关端点统一返回 503
/// - `access_ttl`：access token TTL，humantime 解析（"15m" / "30s"）
/// - `refresh_ttl`：refresh token TTL（控制 Redis 中 opaque token 的 expiry）
/// - `issuer`：写进 `iss` claim 用于多服务区分
#[derive(Debug, Clone, Deserialize)]
pub struct JwtConfig {
    #[serde(default)]
    pub secret: String,
    #[serde(default = "default_access_ttl", with = "humantime_serde")]
    pub access_ttl: Duration,
    #[serde(default = "default_refresh_ttl", with = "humantime_serde")]
    pub refresh_ttl: Duration,
    #[serde(default = "default_issuer")]
    pub issuer: String,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: String::new(),
            access_ttl: default_access_ttl(),
            refresh_ttl: default_refresh_ttl(),
            issuer: default_issuer(),
        }
    }
}

impl JwtConfig {
    /// 仅当 secret 非空时视为"已配置"，可进入 `[jwt]` 启用态。
    pub fn is_configured(&self) -> bool {
        !self.secret.is_empty()
    }
}

/// 签发 / 验证 HS256 access token 的句柄。无状态、可安全 `'static` clone。
///
/// 内部持有 `EncodingKey` / `DecodingKey`（jsonwebtoken crate 预处理后的 secret），
/// 以及 TTL / issuer 元信息。
pub struct JwtSigner {
    encoding_key: jsonwebtoken::EncodingKey,
    decoding_key: jsonwebtoken::DecodingKey,
    validation: jsonwebtoken::Validation,
    access_ttl: Duration,
    refresh_ttl: Duration,
    issuer: String,
}

impl JwtSigner {
    /// 用 `JwtConfig` 构造一个 `JwtSigner`。secret 为空时返回 `NotConfigured`。
    pub fn from_config(cfg: &JwtConfig) -> Result<Self, Error> {
        if !cfg.is_configured() {
            return Err(Error::NotConfigured);
        }
        let secret_bytes = cfg.secret.as_bytes();
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        validation.set_issuer(std::slice::from_ref(&cfg.issuer));
        // 默认 leeway 60s，容忍小幅时钟偏差
        validation.leeway = 60;
        Ok(Self {
            encoding_key: jsonwebtoken::EncodingKey::from_secret(secret_bytes),
            decoding_key: jsonwebtoken::DecodingKey::from_secret(secret_bytes),
            validation,
            access_ttl: cfg.access_ttl,
            refresh_ttl: cfg.refresh_ttl,
            issuer: cfg.issuer.clone(),
        })
    }

    /// access token TTL（用于响应里的 expires_in）。
    pub fn access_ttl(&self) -> Duration {
        self.access_ttl
    }

    /// refresh token TTL（用于 Redis EX 参数）。
    pub fn refresh_ttl(&self) -> Duration {
        self.refresh_ttl
    }

    /// 用 user_id + account + roles + permissions 签发一个 access token。
    pub fn sign_access(
        &self,
        user_id: i64,
        account: &str,
        roles: Vec<String>,
        permissions: Vec<String>,
    ) -> Result<String, Error> {
        let now = tibba_util::timestamp();
        let exp = now + self.access_ttl.as_secs() as i64;
        let claims = Claims {
            sub: user_id,
            account: account.to_string(),
            iss: self.issuer.clone(),
            iat: now,
            exp,
            jti: tibba_util::uuid(),
            roles,
            permissions,
        };
        jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &self.encoding_key,
        )
        .context(SignSnafu)
    }

    /// 验签 + 校验 iss / exp。失败返回 `Verify`（最终 401）。
    pub fn verify_access(&self, token: &str) -> Result<Claims, Error> {
        jsonwebtoken::decode::<Claims>(token, &self.decoding_key, &self.validation)
            .map(|data| data.claims)
            .context(VerifySnafu)
    }
}

/// 启动期一次性设置全局 signer。重复调用返回 Err（拒绝覆盖以避免逻辑错乱）。
/// `JwtSigner` 含 Encoding/Decoding key，体积偏大；返回 Err 时携带原 signer 便于调用方
/// 决定如何处理（通常 panic / 忽略），允许这个 large_err 不影响热路径。
#[allow(clippy::result_large_err)]
pub fn init_global_signer(signer: JwtSigner) -> Result<(), JwtSigner> {
    GLOBAL_SIGNER.set(signer)
}

/// 返回全局 signer 引用；未初始化（[jwt] 未配置）时 None。
pub fn try_global_signer() -> Option<&'static JwtSigner> {
    GLOBAL_SIGNER.get()
}

/// 返回全局 signer 引用；未初始化时 panic。仅在已知 `[jwt]` 已配置时用。
pub fn must_global_signer() -> &'static JwtSigner {
    GLOBAL_SIGNER
        .get()
        .unwrap_or_else(|| panic!("jwt signer not initialized; [jwt] secret missing?"))
}
