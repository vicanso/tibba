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
use ctor::ctor;
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tibba_config::{Config, humantime_serde};
use tibba_error::Error;
use tibba_hook::{BoxFuture, Task, register_task};
use tibba_session::SessionParams;
use tracing::info;
use validator::{Validate, ValidationError};

/// 本模块所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:config=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:config";

type Result<T, E = Error> = std::result::Result<T, E>;
static CONFIGS: OnceCell<Config> = OnceCell::new();

fn map_err(err: impl ToString) -> Error {
    Error::new(err).with_category("config")
}

#[derive(RustEmbed)]
#[folder = "configs/"]
struct Configs;

fn default_commit_id() -> String {
    if let Some(data) = Configs::get("commit_id.txt") {
        std::str::from_utf8(&data.data)
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        "--".to_string()
    }
}

// BasicConfig struct defines the basic application settings
// with validation rules for each field
#[derive(Debug, Clone, Default, Validate, Deserialize)]
pub struct BasicConfig {
    // listen address
    pub listen: String,
    // processing limit
    #[validate(range(min = 0, max = 100000))]
    pub processing_limit: i32,
    // timeout
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
    // secret
    pub secret: String,
    // prefix
    pub prefix: Option<String>,
    // web 静态文件前缀（与 Vite `base` 一致），用于 SPA 部署在子路径下的场景
    pub web_prefix: Option<String>,
    // commit id
    #[serde(default = "default_commit_id")]
    pub commit_id: String,
    // region
    pub region: Option<String>,
}

static BASIC_CONFIG: OnceCell<BasicConfig> = OnceCell::new();

/// Create a new basic config, if the config is invalid, it will panic
fn new_basic_config(config: &Config) -> Result<BasicConfig> {
    let basic_config = config.try_deserialize::<BasicConfig>()?;
    basic_config.validate().map_err(map_err)?;
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

#[derive(Debug, Clone, Default, Validate, Deserialize)]
pub struct SessionConfig {
    // session ttl
    #[serde(with = "humantime_serde")]
    #[validate(custom(function = "validate_session_ttl"))]
    pub ttl: Duration,
    // session secret
    #[validate(length(min = 64))]
    pub secret: String,
    // session cookie name
    #[validate(length(min = 1, max = 64))]
    pub cookie: String,
    // session max renewal
    #[serde(default = "default_session_renewal")]
    #[validate(range(min = 1, max = 52))]
    pub max_renewal: u8,
}

static SESSION_CONFIG: OnceCell<SessionConfig> = OnceCell::new();

// Creates a new SessionConfig instance from the configuration
fn new_session_config(config: &Config) -> Result<SessionConfig> {
    let session_config = config.try_deserialize::<SessionConfig>()?;
    session_config.validate().map_err(map_err)?;
    Ok(session_config)
}

pub fn get_session_params() -> Result<SessionParams> {
    // session config is checked in init function
    let session_config = SESSION_CONFIG
        .get()
        .unwrap_or_else(|| panic!("session config not initialized"));
    let key = Key::try_from(session_config.secret.as_bytes()).map_err(map_err)?;
    Ok(SessionParams::new(key)
        .with_cookie(session_config.cookie.clone())
        .with_ttl(session_config.ttl.as_secs() as i64)
        .with_max_renewal(session_config.max_renewal))
}

#[derive(Debug, Clone, Default, Validate, Deserialize)]
pub struct DivingConfig {
    // diving url
    #[validate(length(min = 1))]
    pub url: String,
    // WeCom robot webhook key (optional)
    pub notify_wecom: Option<String>,
    // 通知接收邮箱地址 (optional)；具体发件 by `tibba_email::EmailService`，配置见 `[email]`
    pub notify_email: Option<String>,
}

static DIVING_CONFIG: OnceCell<DivingConfig> = OnceCell::new();

fn new_diving_config(config: &Config) -> Result<DivingConfig> {
    let diving_config = config.try_deserialize::<DivingConfig>()?;
    diving_config.validate().map_err(map_err)?;
    Ok(diving_config)
}

// 应用全局邮件配置。原 DivingConfig 的 email_from / resend_api_key 抽到此处统一管理，
// 邮箱验证 / 密码重置 / 通知告警等所有发送方共用一份 [email] 配置。
//
// 该段可在 default.toml 中缺失：此时返回 `EmailConfig::default()`（全空），
// 应用照常启动；真正调用 `EmailService::send` 时再以 `Error::Invalid` 报错，
// 避免没用到邮件功能的部署也被强制配置。
static EMAIL_CONFIG: OnceCell<tibba_email::EmailConfig> = OnceCell::new();

fn new_email_config(config: &Config) -> Result<tibba_email::EmailConfig> {
    match config.try_deserialize::<tibba_email::EmailConfig>() {
        Ok(c) => Ok(c),
        // tibba_config 在该段缺失或为空时会报 "missing field"/"not found"，吞掉走默认
        Err(_) => Ok(tibba_email::EmailConfig::default()),
    }
}

// OAuth 全 provider 聚合配置。同样容忍 `[oauth]` 段缺失（→ Default::default()），
// 真正用 OAuth 时由 `GitHubConfig::build_provider` 校验非空，缺失返回 503。
static OAUTH_CONFIG: OnceCell<tibba_oauth::OAuthConfig> = OnceCell::new();

fn new_oauth_config(config: &Config) -> Result<tibba_oauth::OAuthConfig> {
    match config.try_deserialize::<tibba_oauth::OAuthConfig>() {
        Ok(c) => Ok(c),
        Err(_) => Ok(tibba_oauth::OAuthConfig::default()),
    }
}

// JWT 备选鉴权配置。整段缺失或 secret 为空时应用照常启动，
// 仅 JWT 端点（/login/jwt 等）和 JwtUser extractor 返回 503。
static JWT_CONFIG: OnceCell<tibba_jwt::JwtConfig> = OnceCell::new();

fn new_jwt_config(config: &Config) -> Result<tibba_jwt::JwtConfig> {
    match config.try_deserialize::<tibba_jwt::JwtConfig>() {
        Ok(c) => Ok(c),
        Err(_) => Ok(tibba_jwt::JwtConfig::default()),
    }
}

#[derive(Debug, Clone, Default, Validate, Deserialize)]
pub struct TokenConfig {
    /// 可选模型名列表，供 token_llm / token_price 的 `model` 字段下拉展示。
    #[serde(default)]
    pub models: Vec<String>,
}

static TOKEN_CONFIG: OnceCell<TokenConfig> = OnceCell::new();

fn new_token_config(config: &Config) -> Result<TokenConfig> {
    let token_config = config.try_deserialize::<TokenConfig>()?;
    token_config.validate().map_err(map_err)?;
    Ok(token_config)
}

pub fn must_get_token_config() -> &'static TokenConfig {
    TOKEN_CONFIG
        .get()
        .unwrap_or_else(|| panic!("token config not initialized"))
}
fn new_config() -> Result<&'static Config> {
    CONFIGS.get_or_try_init(|| {
        let mut arr = vec![];
        for name in ["default.toml", &format!("{}.toml", tibba_util::get_env())] {
            let data = Configs::get(name)
                .ok_or(map_err(format!("{name} not found")))?
                .data;
            info!(target: LOG_TARGET, "load config from {name}");
            arr.push(std::string::String::from_utf8_lossy(&data).to_string());
        }

        let data: Vec<&str> = arr.iter().map(|s| s.as_str()).collect();
        let config = tibba_config::Config::new(&data, Some("TIBBA_WEB"))?;
        Ok(config)
    })
}

pub fn must_get_config() -> &'static Config {
    new_config().unwrap_or_else(|_| panic!("config not initialized"))
}

pub fn must_get_basic_config() -> &'static BasicConfig {
    BASIC_CONFIG
        .get()
        .unwrap_or_else(|| panic!("basic config not initialized"))
}

pub fn must_get_diving_config() -> &'static DivingConfig {
    DIVING_CONFIG
        .get()
        .unwrap_or_else(|| panic!("diving config not initialized"))
}

pub fn must_get_email_config() -> &'static tibba_email::EmailConfig {
    EMAIL_CONFIG
        .get()
        .unwrap_or_else(|| panic!("email config not initialized"))
}

pub fn must_get_oauth_config() -> &'static tibba_oauth::OAuthConfig {
    OAUTH_CONFIG
        .get()
        .unwrap_or_else(|| panic!("oauth config not initialized"))
}

// 暂留 API 供后续 admin / health 端点查询 JWT 启用态；初始化只在 init_config 内部消费 JwtConfig
#[allow(dead_code)]
pub fn must_get_jwt_config() -> &'static tibba_jwt::JwtConfig {
    JWT_CONFIG
        .get()
        .unwrap_or_else(|| panic!("jwt config not initialized"))
}

async fn init_config() -> Result<()> {
    let app_config = new_config()?;
    let basic_config = new_basic_config(&app_config.sub_config("basic"))?;
    BASIC_CONFIG
        .set(basic_config)
        .map_err(|_| map_err("basic config init failed"))?;
    let session_config = new_session_config(&app_config.sub_config("session"))?;
    SESSION_CONFIG
        .set(session_config)
        .map_err(|_| map_err("session config init failed"))?;
    let diving_config = new_diving_config(&app_config.sub_config("diving"))?;
    DIVING_CONFIG
        .set(diving_config)
        .map_err(|_| map_err("diving config init failed"))?;
    let email_config = new_email_config(&app_config.sub_config("email"))?;
    EMAIL_CONFIG
        .set(email_config)
        .map_err(|_| map_err("email config init failed"))?;
    let oauth_config = new_oauth_config(&app_config.sub_config("oauth"))?;
    OAUTH_CONFIG
        .set(oauth_config)
        .map_err(|_| map_err("oauth config init failed"))?;
    let jwt_config = new_jwt_config(&app_config.sub_config("jwt"))?;
    // 若 [jwt] 已配 secret，启动期一次性把全局 signer 初始化好；未配则跳过
    // （延迟到首次访问 JWT 端点时由 try_global_signer 返回 None → 503）
    if jwt_config.is_configured() {
        let signer = tibba_jwt::JwtSigner::from_config(&jwt_config).map_err(map_err)?;
        tibba_jwt::init_global_signer(signer)
            .map_err(|_| map_err("jwt global signer already initialized"))?;
    }
    JWT_CONFIG
        .set(jwt_config)
        .map_err(|_| map_err("jwt config init failed"))?;
    let token_config = new_token_config(&app_config.sub_config("token"))?;
    TOKEN_CONFIG
        .set(token_config)
        .map_err(|_| map_err("token config init failed"))?;
    Ok(())
}

struct ConfigTask;

impl Task for ConfigTask {
    fn before(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            init_config().await?;
            Ok(true)
        })
    }
}

// add application init before application start
#[ctor(unsafe)]
fn init() {
    register_task("config", Arc::new(ConfigTask));
}
