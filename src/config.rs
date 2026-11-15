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

use axum_extra::extract::cookie::Key;
use serde::Deserialize;
use std::borrow::Cow;
use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;
use tibba_auth::SessionParams;
#[cfg(feature = "jwt")]
use tibba_auth::jwt::{JwtConfig, JwtSigner, init_global_signer};
#[cfg(feature = "oauth")]
use tibba_auth::oauth::OAuthConfig;
use tibba_config::{Config, humantime_serde};
use tibba_error::Error;
use tibba_util::{is_development, is_test};
use tracing::info;
use validator::{Validate, ValidationError};

/// 脚手架自带的开发用示例 `basic.secret`（见 configs/default.toml）。
/// 生产环境必须覆盖（`TIBBA_WEB__BASIC__SECRET`），否则启动校验失败。
const DEV_PLACEHOLDER_SECRET: &str = "tibba-dev-insecure-secret-do-not-use-in-production";

/// 本模块所有日志事件的 tracing target。
const LOG_TARGET: &str = "tibba:config";

type Result<T, E = Error> = std::result::Result<T, E>;
static CONFIGS: OnceLock<Config> = OnceLock::new();

fn embedded_config(name: &str) -> Option<&'static str> {
    match name {
        "default.toml" => Some(include_str!("../configs/default.toml")),
        "dev.toml" => Some(include_str!("../configs/dev.toml")),
        "production.toml" => Some(include_str!("../configs/production.toml")),
        "test.toml" => Some(include_str!("../configs/test.toml")),
        _ => None,
    }
}

/// 将配置校验 / 反序列化失败包装为 `tibba_error::Error`（category=config）。
fn config_error(err: impl ToString) -> Error {
    Error::new(err).with_category("config")
}

fn default_commit_id() -> String {
    let raw = include_str!("../configs/commit_id.txt").trim();
    if raw.is_empty() {
        "--".to_string()
    } else {
        raw.to_string()
    }
}

fn redact_secret(value: &str) -> &'static str {
    if value.is_empty() { "" } else { "[REDACTED]" }
}

/// 基础应用配置。
#[derive(Clone, Default, Validate, Deserialize)]
pub struct BasicConfig {
    /// 监听地址
    #[validate(custom(function = "tibba_util::x_listen_addr"))]
    pub listen: String,
    /// 最大并发请求数，0 表示不限制
    #[validate(range(max = 100000))]
    pub processing_limit: u32,
    /// 请求超时时间
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
    /// 应用密钥（派生 TOTP 密钥、签名登录防重放令牌，生产禁止用示例值）
    #[validate(length(min = 32), custom(function = "validate_basic_secret"))]
    pub secret: String,
    /// 可选密码哈希全局 pepper，配置后会在启动期注入 `tibba_crypto::PasswordPolicy`。
    #[serde(default)]
    pub password_pepper: Option<String>,
    /// API 路由前缀
    pub prefix: Option<String>,
    /// Web 静态文件前缀（与 Vite `base` 一致）
    pub web_prefix: Option<String>,
    /// Git commit ID
    #[serde(default = "default_commit_id")]
    pub commit_id: String,
    /// 生产 CORS 来源白名单（生产环境必须非空）
    #[serde(default)]
    pub cors_allow_origins: Vec<String>,
    /// 是否允许跨域携带凭证
    #[serde(default)]
    pub cors_allow_credentials: bool,
    /// 启动时授予超级管理员角色的账号列表（幂等）
    #[serde(default)]
    pub super_admins: Vec<String>,
}

impl fmt::Debug for BasicConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BasicConfig")
            .field("listen", &self.listen)
            .field("processing_limit", &self.processing_limit)
            .field("timeout", &self.timeout)
            .field("secret", &redact_secret(&self.secret))
            .field(
                "password_pepper",
                &self.password_pepper.as_deref().map(redact_secret),
            )
            .field("prefix", &self.prefix)
            .field("web_prefix", &self.web_prefix)
            .field("commit_id", &self.commit_id)
            .field("cors_allow_origins", &self.cors_allow_origins)
            .field("cors_allow_credentials", &self.cors_allow_credentials)
            .field("super_admins", &self.super_admins)
            .finish()
    }
}

impl BasicConfig {
    /// 将 `listen` 配置归一化为可直接用于 `TcpListener::bind` 的地址；
    /// 将 `:port` 简写补齐为 `0.0.0.0:port`。
    #[must_use]
    pub fn bind_addr(&self) -> Cow<'_, str> {
        if let Some(port) = self.listen.strip_prefix(':') {
            Cow::Owned(format!("0.0.0.0:{port}"))
        } else {
            Cow::Borrowed(&self.listen)
        }
    }
}

static BASIC_CONFIG: OnceLock<BasicConfig> = OnceLock::new();

/// 校验 basic.secret：生产环境禁止使用脚手架自带的开发示例值。
fn validate_basic_secret(secret: &str) -> Result<(), ValidationError> {
    if !is_development() && !is_test() && secret == DEV_PLACEHOLDER_SECRET {
        return Err(ValidationError::new(
            "basic.secret must be overridden in production (set TIBBA_WEB__BASIC__SECRET)",
        ));
    }
    Ok(())
}

fn new_basic_config(config: &Config) -> Result<BasicConfig> {
    let basic_config = config.try_deserialize::<BasicConfig>()?;
    basic_config.validate().map_err(config_error)?;
    Ok(basic_config)
}

fn validate_session_ttl(ttl: &Duration) -> Result<(), ValidationError> {
    if ttl.as_secs() < 60 {
        return Err(ValidationError::new("session ttl is too short"));
    }
    if ttl.as_secs() > 2592000 {
        return Err(ValidationError::new("session ttl is too long"));
    }
    Ok(())
}

fn default_session_renewal() -> u8 {
    52
}

#[derive(Clone, Default, Validate, Deserialize)]
pub struct SessionConfig {
    #[serde(with = "humantime_serde")]
    #[validate(custom(function = "validate_session_ttl"))]
    pub ttl: Duration,
    #[validate(length(min = 64))]
    pub secret: String,
    #[validate(length(min = 1, max = 64))]
    pub cookie: String,
    #[serde(default = "default_session_renewal")]
    #[validate(range(min = 1, max = 52))]
    pub max_renewal: u8,
}

impl fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionConfig")
            .field("ttl", &self.ttl)
            .field("secret", &redact_secret(&self.secret))
            .field("cookie", &self.cookie)
            .field("max_renewal", &self.max_renewal)
            .finish()
    }
}

static SESSION_CONFIG: OnceLock<SessionConfig> = OnceLock::new();

fn new_session_config(config: &Config) -> Result<SessionConfig> {
    let session_config = config.try_deserialize::<SessionConfig>()?;
    session_config.validate().map_err(config_error)?;
    Ok(session_config)
}

pub fn get_session_params() -> Result<SessionParams> {
    let session_config = SESSION_CONFIG
        .get()
        .unwrap_or_else(|| panic!("session config not initialized"));
    let key = Key::try_from(session_config.secret.as_bytes()).map_err(config_error)?;
    Ok(SessionParams::new(key)
        .with_cookie(session_config.cookie.clone())
        .with_ttl(session_config.ttl.as_secs() as i64)
        .with_max_renewal(session_config.max_renewal))
}

static EMAIL_CONFIG: OnceLock<tibba_email::EmailConfig> = OnceLock::new();

fn new_email_config(app_config: &Config) -> Result<tibba_email::EmailConfig> {
    #[derive(Default, Deserialize)]
    struct Section {
        #[serde(default)]
        email: tibba_email::EmailConfig,
    }
    Ok(app_config.try_deserialize::<Section>()?.email)
}

#[cfg(feature = "oauth")]
static OAUTH_CONFIG: OnceLock<OAuthConfig> = OnceLock::new();

#[cfg(feature = "oauth")]
fn new_oauth_config(app_config: &Config) -> Result<OAuthConfig> {
    #[derive(Default, Deserialize)]
    struct Section {
        #[serde(default)]
        oauth: OAuthConfig,
    }
    Ok(app_config.try_deserialize::<Section>()?.oauth)
}

#[cfg(feature = "jwt")]
static JWT_CONFIG: OnceLock<JwtConfig> = OnceLock::new();

#[cfg(feature = "jwt")]
fn new_jwt_config(app_config: &Config) -> Result<JwtConfig> {
    #[derive(Default, Deserialize)]
    struct Section {
        #[serde(default)]
        jwt: JwtConfig,
    }
    let jwt_config = app_config.try_deserialize::<Section>()?.jwt;
    // 启用 JWT（secret 非空）时强制 >=32 字符
    if jwt_config.is_configured() && jwt_config.secret.len() < 32 {
        return Err(Error::new(
            "jwt.secret must be at least 32 chars when configured (set TIBBA_WEB__JWT__SECRET)",
        )
        .with_category("config"));
    }
    Ok(jwt_config)
}

fn load_config() -> Result<&'static Config> {
    if let Some(config) = CONFIGS.get() {
        return Ok(config);
    }
    // default.toml 打底，{ENV}.toml 覆盖，环境变量 TIBBA_WEB__ 优先级最高
    let mut builder = Config::builder().with_env_prefix("TIBBA_WEB");
    for name in ["default.toml", &format!("{}.toml", tibba_util::get_env())] {
        let data =
            embedded_config(name).ok_or_else(|| config_error(format!("{name} not found")))?;
        info!(target: LOG_TARGET, "load config from {name}");
        builder = builder.add_toml(data);
    }
    let config = builder.build()?;
    let _ = CONFIGS.set(config);
    CONFIGS
        .get()
        .ok_or_else(|| config_error("config not initialized"))
}

pub fn must_get_config() -> &'static Config {
    CONFIGS
        .get()
        .unwrap_or_else(|| panic!("config not initialized"))
}

pub fn must_get_basic_config() -> &'static BasicConfig {
    BASIC_CONFIG
        .get()
        .unwrap_or_else(|| panic!("basic config not initialized"))
}

pub fn must_get_email_config() -> &'static tibba_email::EmailConfig {
    EMAIL_CONFIG
        .get()
        .unwrap_or_else(|| panic!("email config not initialized"))
}

#[cfg(feature = "oauth")]
pub fn must_get_oauth_config() -> &'static OAuthConfig {
    OAUTH_CONFIG
        .get()
        .unwrap_or_else(|| panic!("oauth config not initialized"))
}

#[cfg(feature = "jwt")]
#[allow(dead_code)]
pub fn must_get_jwt_config() -> &'static JwtConfig {
    JWT_CONFIG
        .get()
        .unwrap_or_else(|| panic!("jwt config not initialized"))
}

/// 显式初始化所有应用配置段与全局安全策略；由 `main.rs` 在启动期调用。
pub fn init_config() -> Result<()> {
    let app_config = load_config()?;

    let basic_config = new_basic_config(&app_config.sub_config("basic"))?;
    BASIC_CONFIG
        .set(basic_config)
        .map_err(|_| config_error("basic config init failed"))?;
    if let Some(pepper) = must_get_basic_config()
        .password_pepper
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        let policy = tibba_crypto::PasswordPolicy::default().with_pepper(pepper.as_bytes());
        tibba_crypto::init_global_password_policy(policy).map_err(config_error)?;
    }

    let session_config = new_session_config(&app_config.sub_config("session"))?;
    SESSION_CONFIG
        .set(session_config)
        .map_err(|_| config_error("session config init failed"))?;

    let email_config = new_email_config(app_config)?;
    EMAIL_CONFIG
        .set(email_config)
        .map_err(|_| config_error("email config init failed"))?;

    #[cfg(feature = "oauth")]
    {
        let oauth_config = new_oauth_config(app_config)?;
        OAUTH_CONFIG
            .set(oauth_config)
            .map_err(|_| config_error("oauth config init failed"))?;
    }

    #[cfg(feature = "jwt")]
    {
        let jwt_config = new_jwt_config(app_config)?;
        if jwt_config.is_configured() {
            if let Some(session) = SESSION_CONFIG.get()
                && (jwt_config.access_ttl > session.ttl || jwt_config.refresh_ttl > session.ttl)
            {
                return Err(config_error(
                    "jwt.access_ttl and jwt.refresh_ttl must not exceed session.ttl",
                ));
            }
            let signer = JwtSigner::from_config(&jwt_config).map_err(config_error)?;
            init_global_signer(signer)
                .map_err(|_| config_error("jwt global signer already initialized"))?;
        }
        JWT_CONFIG
            .set(jwt_config)
            .map_err(|_| config_error("jwt config init failed"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_config_bind_addr_and_listen_validation() {
        let mut cfg = BasicConfig {
            listen: ":5000".to_string(),
            processing_limit: 100,
            timeout: Duration::from_secs(30),
            secret: "01234567890123456789012345678901".to_string(),
            ..Default::default()
        };
        assert_eq!(cfg.bind_addr(), "0.0.0.0:5000");
        assert!(cfg.validate().is_ok());

        cfg.listen = "127.0.0.1:5000".to_string();
        assert_eq!(cfg.bind_addr(), "127.0.0.1:5000");
        assert!(cfg.validate().is_ok());

        cfg.listen = "not-an-addr".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn configs_redact_secrets_in_debug() {
        let basic = BasicConfig {
            listen: "127.0.0.1:5000".to_string(),
            secret: "super-basic-secret-0123456789012345".to_string(),
            password_pepper: Some("super-pepper-value".to_string()),
            ..Default::default()
        };
        let basic_dbg = format!("{basic:?}");
        assert!(!basic_dbg.contains("super-basic-secret"), "{basic_dbg}");
        assert!(!basic_dbg.contains("super-pepper-value"), "{basic_dbg}");
        assert!(basic_dbg.contains("[REDACTED]"), "{basic_dbg}");

        let session = SessionConfig {
            ttl: Duration::from_secs(3600),
            secret: "session-secret-0123456789".to_string(),
            cookie: "sid".to_string(),
            max_renewal: 10,
        };
        let session_dbg = format!("{session:?}");
        assert!(!session_dbg.contains("session-secret"), "{session_dbg}");
        assert!(session_dbg.contains("[REDACTED]"), "{session_dbg}");
    }

    #[test]
    fn optional_config_sections_default_when_missing_and_fail_fast_on_malformed() {
        let empty = Config::builder().add_toml("").build().expect("empty cfg");
        assert!(new_email_config(&empty).is_ok());

        let bad_email = Config::builder()
            .add_toml("email = 42")
            .build()
            .expect("cfg");
        assert!(new_email_config(&bad_email).is_err());

        #[cfg(feature = "oauth")]
        {
            assert!(new_oauth_config(&empty).is_ok());
            let bad_oauth = Config::builder()
                .add_toml("oauth = \"invalid\"")
                .build()
                .expect("cfg");
            assert!(new_oauth_config(&bad_oauth).is_err());
        }

        #[cfg(feature = "jwt")]
        {
            assert!(new_jwt_config(&empty).is_ok());
            let bad_jwt = Config::builder()
                .add_toml("[jwt]\naccess_ttl = \"not-a-duration\"")
                .build()
                .expect("cfg");
            assert!(new_jwt_config(&bad_jwt).is_err());
        }
    }
}
