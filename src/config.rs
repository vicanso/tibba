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
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tibba_config::{Config, humantime_serde};
use tibba_error::Error;
use tibba_hook::{BoxFuture, Task, register_task};
use tibba_session::SessionParams;
use tibba_util::{is_development, is_test};
use tracing::info;
use validator::{Validate, ValidationError};

/// 脚手架自带的开发用示例 `basic.secret`（见 configs/default.toml）。
/// 生产环境必须覆盖（`TIBBA_WEB__BASIC__SECRET`），否则启动校验失败。
const DEV_PLACEHOLDER_SECRET: &str = "tibba-dev-insecure-secret-do-not-use-in-production";

/// 本模块所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:config=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:config";

type Result<T, E = Error> = std::result::Result<T, E>;
static CONFIGS: OnceLock<Config> = OnceLock::new();

/// 将配置校验 / 反序列化失败包装为 `tibba_error::Error`（category=config）。
/// 配置模块错误源多为字符串化校验信息，无独立 external Error 类型可做 snafu source。
fn config_error(err: impl ToString) -> Error {
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
    // secret：派生 TOTP AES-256-GCM 密钥、签发登录防重放令牌，必须足够长且生产不得用示例值
    #[validate(length(min = 32), custom(function = "validate_basic_secret"))]
    pub secret: String,
    // prefix
    pub prefix: Option<String>,
    // web 静态文件前缀（与 Vite `base` 一致），用于 SPA 部署在子路径下的场景
    pub web_prefix: Option<String>,
    // commit id
    #[serde(default = "default_commit_id")]
    pub commit_id: String,
    /// 部署区域标签（HTTP 探测等样板任务按 region 过滤；核心路径可不读）
    #[allow(dead_code)]
    pub region: Option<String>,
    /// 生产 CORS 来源白名单（如 `https://app.example.com`）。
    /// 生产环境**必须**非空，否则启动 fail-fast（见 `Cors::assert_production_safe`）。
    /// 环境变量：`TIBBA_WEB__BASIC__CORS_ALLOW_ORIGINS`（TOML 数组 / 配置层 list）。
    #[serde(default)]
    pub cors_allow_origins: Vec<String>,
    /// 是否允许跨域携带 Cookie / Authorization（与白名单配合使用）。
    #[serde(default)]
    pub cors_allow_credentials: bool,
}

static BASIC_CONFIG: OnceLock<BasicConfig> = OnceLock::new();

/// 校验 basic.secret：生产环境禁止使用脚手架自带的开发示例值。
/// dev / test 允许沿用示例值以便开箱即用；长度下限由 `#[validate(length)]` 单独保证。
fn validate_basic_secret(secret: &str) -> Result<(), ValidationError> {
    if !is_development() && !is_test() && secret == DEV_PLACEHOLDER_SECRET {
        return Err(ValidationError::new(
            "basic.secret must be overridden in production (set TIBBA_WEB__BASIC__SECRET)",
        ));
    }
    Ok(())
}

/// Create a new basic config, if the config is invalid, it will panic
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

static SESSION_CONFIG: OnceLock<SessionConfig> = OnceLock::new();

// Creates a new SessionConfig instance from the configuration
fn new_session_config(config: &Config) -> Result<SessionConfig> {
    let session_config = config.try_deserialize::<SessionConfig>()?;
    session_config.validate().map_err(config_error)?;
    Ok(session_config)
}

pub fn get_session_params() -> Result<SessionParams> {
    // session config is checked in init function
    let session_config = SESSION_CONFIG
        .get()
        .unwrap_or_else(|| panic!("session config not initialized"));
    let key = Key::try_from(session_config.secret.as_bytes()).map_err(config_error)?;
    Ok(SessionParams::new(key)
        .with_cookie(session_config.cookie.clone())
        .with_ttl(session_config.ttl.as_secs() as i64)
        .with_max_renewal(session_config.max_renewal))
}

/// Docker 镜像分析（diving）服务配置；仅 `demo-docker` 路径消费。
#[derive(Debug, Clone, Default, Validate, Deserialize)]
#[cfg_attr(not(feature = "demo-docker"), allow(dead_code))]
pub struct DivingConfig {
    // diving url
    #[validate(length(min = 1))]
    pub url: String,
    // WeCom robot webhook key (optional)
    pub notify_wecom: Option<String>,
    // 通知接收邮箱地址 (optional)；具体发件 by `tibba_email::EmailService`，配置见 `[email]`
    pub notify_email: Option<String>,
}

static DIVING_CONFIG: OnceLock<DivingConfig> = OnceLock::new();

fn new_diving_config(config: &Config) -> Result<DivingConfig> {
    let diving_config = config.try_deserialize::<DivingConfig>()?;
    diving_config.validate().map_err(config_error)?;
    Ok(diving_config)
}

// 应用全局邮件配置。原 DivingConfig 的 email_from / resend_api_key 抽到此处统一管理，
// 邮箱验证 / 密码重置 / 通知告警等所有发送方共用一份 [email] 配置。
//
// 该段可在 default.toml 中缺失：此时返回 `EmailConfig::default()`（全空），
// 应用照常启动；真正调用 `EmailService::send` 时再以 `Error::Invalid` 报错，
// 避免没用到邮件功能的部署也被强制配置。
static EMAIL_CONFIG: OnceLock<tibba_email::EmailConfig> = OnceLock::new();

fn new_email_config(config: &Config) -> Result<tibba_email::EmailConfig> {
    match config.try_deserialize::<tibba_email::EmailConfig>() {
        Ok(c) => Ok(c),
        // tibba_config 在该段缺失或为空时会报 "missing field"/"not found"，吞掉走默认
        Err(_) => Ok(tibba_email::EmailConfig::default()),
    }
}

// OAuth 全 provider 聚合配置。同样容忍 `[oauth]` 段缺失（→ Default::default()），
// 真正用 OAuth 时由 `GitHubConfig::build_provider` 校验非空，缺失返回 503。
static OAUTH_CONFIG: OnceLock<tibba_oauth::OAuthConfig> = OnceLock::new();

fn new_oauth_config(config: &Config) -> Result<tibba_oauth::OAuthConfig> {
    match config.try_deserialize::<tibba_oauth::OAuthConfig>() {
        Ok(c) => Ok(c),
        Err(_) => Ok(tibba_oauth::OAuthConfig::default()),
    }
}

// JWT 备选鉴权配置。整段缺失或 secret 为空时应用照常启动，
// 仅 JWT 端点（/login/jwt 等）和 JwtUser extractor 返回 503。
static JWT_CONFIG: OnceLock<tibba_jwt::JwtConfig> = OnceLock::new();

fn new_jwt_config(config: &Config) -> Result<tibba_jwt::JwtConfig> {
    let jwt_config = config
        .try_deserialize::<tibba_jwt::JwtConfig>()
        .unwrap_or_default();
    // 启用 JWT（secret 非空）时强制 ≥32 字符；短 HS256 密钥可离线爆破伪造 token（越权）。
    // 空 secret 表示未启用，照常放行（相关端点返回 503）。
    if jwt_config.is_configured() && jwt_config.secret.len() < 32 {
        return Err(Error::new(
            "jwt.secret must be at least 32 chars when configured (set TIBBA_WEB__JWT__SECRET)",
        )
        .with_category("config"));
    }
    Ok(jwt_config)
}

#[derive(Debug, Clone, Default, Validate, Deserialize)]
pub struct TokenConfig {
    /// 可选模型名列表，供 token_llm / token_price 的 `model` 字段下拉展示。
    #[serde(default)]
    pub models: Vec<String>,
}

static TOKEN_CONFIG: OnceLock<TokenConfig> = OnceLock::new();

fn new_token_config(config: &Config) -> Result<TokenConfig> {
    let token_config = config.try_deserialize::<TokenConfig>()?;
    token_config.validate().map_err(config_error)?;
    Ok(token_config)
}

pub fn must_get_token_config() -> &'static TokenConfig {
    TOKEN_CONFIG
        .get()
        .unwrap_or_else(|| panic!("token config not initialized"))
}

// 出站 webhook 配置。整段缺失或 secret 为空时应用照常启动，投递不签名。
#[derive(Debug, Clone, Default, Validate, Deserialize)]
pub struct WebhookConfig {
    /// 出站 webhook 的 HMAC-SHA256 签名密钥；为空则投递不签名。
    /// 经环境变量 `TIBBA_WEB__WEBHOOK__SECRET` 注入，切勿写入仓库。
    #[serde(default)]
    pub secret: String,
}

static WEBHOOK_CONFIG: OnceLock<WebhookConfig> = OnceLock::new();

fn new_webhook_config(config: &Config) -> Result<WebhookConfig> {
    // 整段缺失视为未配置（不签名），与 jwt / oauth 的可选语义一致
    match config.try_deserialize::<WebhookConfig>() {
        Ok(c) => Ok(c),
        Err(_) => Ok(WebhookConfig::default()),
    }
}

pub fn must_get_webhook_config() -> &'static WebhookConfig {
    WEBHOOK_CONFIG
        .get()
        .unwrap_or_else(|| panic!("webhook config not initialized"))
}
fn new_config() -> Result<&'static Config> {
    // OnceLock::get_or_try_init 尚未稳定；先 get，未初始化再加载 + set
    if let Some(config) = CONFIGS.get() {
        return Ok(config);
    }
    let mut arr = vec![];
    for name in ["default.toml", &format!("{}.toml", tibba_util::get_env())] {
        let data = Configs::get(name)
            .ok_or(config_error(format!("{name} not found")))?
            .data;
        info!(target: LOG_TARGET, "load config from {name}");
        arr.push(std::string::String::from_utf8_lossy(&data).to_string());
    }

    let data: Vec<&str> = arr.iter().map(|s| s.as_str()).collect();
    let config = tibba_config::Config::new(&data, Some("TIBBA_WEB"))?;
    let _ = CONFIGS.set(config);
    CONFIGS
        .get()
        .ok_or_else(|| config_error("config not initialized"))
}

pub fn must_get_config() -> &'static Config {
    new_config().unwrap_or_else(|_| panic!("config not initialized"))
}

pub fn must_get_basic_config() -> &'static BasicConfig {
    BASIC_CONFIG
        .get()
        .unwrap_or_else(|| panic!("basic config not initialized"))
}

#[cfg_attr(not(feature = "demo-docker"), allow(dead_code))]
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
        .map_err(|_| config_error("basic config init failed"))?;
    let session_config = new_session_config(&app_config.sub_config("session"))?;
    SESSION_CONFIG
        .set(session_config)
        .map_err(|_| config_error("session config init failed"))?;
    let diving_config = new_diving_config(&app_config.sub_config("diving"))?;
    DIVING_CONFIG
        .set(diving_config)
        .map_err(|_| config_error("diving config init failed"))?;
    let email_config = new_email_config(&app_config.sub_config("email"))?;
    EMAIL_CONFIG
        .set(email_config)
        .map_err(|_| config_error("email config init failed"))?;
    let oauth_config = new_oauth_config(&app_config.sub_config("oauth"))?;
    OAUTH_CONFIG
        .set(oauth_config)
        .map_err(|_| config_error("oauth config init failed"))?;
    let jwt_config = new_jwt_config(&app_config.sub_config("jwt"))?;
    // 若 [jwt] 已配 secret，启动期一次性把全局 signer 初始化好；未配则跳过
    // （延迟到首次访问 JWT 端点时由 try_global_signer 返回 None → 503）
    if jwt_config.is_configured() {
        let signer = tibba_jwt::JwtSigner::from_config(&jwt_config).map_err(config_error)?;
        tibba_jwt::init_global_signer(signer)
            .map_err(|_| config_error("jwt global signer already initialized"))?;
    }
    JWT_CONFIG
        .set(jwt_config)
        .map_err(|_| config_error("jwt config init failed"))?;
    let token_config = new_token_config(&app_config.sub_config("token"))?;
    TOKEN_CONFIG
        .set(token_config)
        .map_err(|_| config_error("token config init failed"))?;
    let webhook_config = new_webhook_config(&app_config.sub_config("webhook"))?;
    WEBHOOK_CONFIG
        .set(webhook_config)
        .map_err(|_| config_error("webhook config init failed"))?;
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
