use clap::{Parser, Subcommand};
use inquire::MultiSelect;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const TIBBA_VERSION: &str = "0.2.0";

/// 将路径中的 `~` 前缀展开为实际的 home 目录。
fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

// ── 可选模块定义 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Feature {
    Sql,
    Cache,
    Session,
    Opendal,
    Scheduler,
    Headless,
    RouterCommon,
    RouterUser,
    RouterFile,
    RouterModel,
    // 自动依赖，不直接展示给用户选择
    Model,
}

impl Feature {
    fn selectable() -> Vec<(&'static str, Feature)> {
        vec![
            ("sql           - PostgreSQL 数据库", Feature::Sql),
            ("cache         - Redis 缓存", Feature::Cache),
            ("session       - 会话管理（依赖 cache）", Feature::Session),
            ("opendal       - 文件存储（S3 / 本地 / HTTP）", Feature::Opendal),
            ("scheduler     - 定时任务（cron）", Feature::Scheduler),
            ("headless      - 无头浏览器", Feature::Headless),
            ("router-common - 公共路由（ping / captcha，依赖 cache）", Feature::RouterCommon),
            ("router-user   - 用户路由（登录 / 注册，依赖 sql / cache / session）", Feature::RouterUser),
            ("router-file   - 文件路由（上传 / 下载，依赖 sql / opendal / session）", Feature::RouterFile),
            ("router-model  - 模型 CRUD 路由（依赖 sql / session）", Feature::RouterModel),
        ]
    }

    fn from_str(s: &str) -> Option<Feature> {
        match s {
            "sql" => Some(Feature::Sql),
            "cache" => Some(Feature::Cache),
            "session" => Some(Feature::Session),
            "opendal" => Some(Feature::Opendal),
            "scheduler" => Some(Feature::Scheduler),
            "headless" => Some(Feature::Headless),
            "router-common" => Some(Feature::RouterCommon),
            "router-user" => Some(Feature::RouterUser),
            "router-file" => Some(Feature::RouterFile),
            "router-model" => Some(Feature::RouterModel),
            _ => None,
        }
    }
}

/// 将用户选择的模块补全为完整的传递依赖集合。
fn resolve(selected: HashSet<Feature>) -> HashSet<Feature> {
    let mut f = selected;

    if f.contains(&Feature::Session) {
        f.insert(Feature::Cache);
    }
    if f.contains(&Feature::RouterCommon) {
        f.insert(Feature::Cache);
    }
    if f.contains(&Feature::RouterUser) {
        f.insert(Feature::Sql);
        f.insert(Feature::Cache);
        f.insert(Feature::Session);
        f.insert(Feature::Model);
    }
    if f.contains(&Feature::RouterFile) {
        f.insert(Feature::Sql);
        f.insert(Feature::Opendal);
        f.insert(Feature::Session);
        f.insert(Feature::Model);
    }
    if f.contains(&Feature::RouterModel) {
        f.insert(Feature::Sql);
        f.insert(Feature::Session);
        f.insert(Feature::Model);
    }
    // session 补充 cache（可能在上面刚插入 session）
    if f.contains(&Feature::Session) {
        f.insert(Feature::Cache);
    }
    f
}

// ── 模块依赖图（源自 docs/modules.md） ───────────────────────────────────────

/// 返回指定 tibba-* 模块的直接依赖列表。
fn module_deps(module: &str) -> &'static [&'static str] {
    match module {
        "error" | "state" | "performance" | "validator" => &[],
        "util" | "config" | "crypto" | "headless" | "hook" | "model" | "scheduler" => {
            &["error"]
        }
        "cache" | "opendal" | "sql" => &["config", "error", "util"],
        "request" => &["error", "util"],
        "middleware" => &["cache", "error", "state", "util"],
        "router-common" => &["cache", "error", "performance", "state", "util"],
        "session" => &["cache", "error", "state", "util"],
        "router-file" => &["error", "model", "opendal", "session", "util", "validator"],
        "router-model" => &["error", "model", "session", "util", "validator"],
        "router-user" => &[
            "cache", "error", "middleware", "model", "session", "util", "validator",
        ],
        _ => &[],
    }
}

/// 对给定模块集合求传递闭包，返回所有需要的 tibba-* 模块名（已排序）。
fn transitive_modules(roots: &[&str]) -> Vec<String> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<&str> = roots.to_vec();
    while let Some(m) = stack.pop() {
        if visited.insert(m.to_string()) {
            stack.extend_from_slice(module_deps(m));
        }
    }
    let mut list: Vec<String> = visited.into_iter().collect();
    list.sort();
    list
}

fn feature_modules(feature: &Feature) -> &'static [&'static str] {
    match feature {
        Feature::Sql => &["sql"],
        Feature::Cache => &["cache"],
        Feature::Session => &["session"],
        Feature::Opendal => &["opendal"],
        Feature::Scheduler => &["scheduler"],
        Feature::Headless => &["headless"],
        Feature::RouterCommon => &["router-common"],
        Feature::RouterUser => &["router-user"],
        Feature::RouterFile => &["router-file"],
        Feature::RouterModel => &["router-model"],
        Feature::Model => &["model"],
    }
}

/// 根据选中的 Feature 集合，计算完整的 tibba-* 模块列表（含传递依赖）。
fn all_tibba_modules(features: &HashSet<Feature>) -> Vec<String> {
    let mut roots: Vec<&str> = vec!["error", "state", "hook", "util", "config", "middleware"];
    for f in features {
        roots.extend_from_slice(feature_modules(f));
    }
    transitive_modules(&roots)
}

// ── CLI 定义 ──────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "tibba-scaffold", about = "tibba 项目脚手架生成工具")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 创建新的 tibba web server 项目
    New {
        /// 项目名称（同时作为 Cargo package 名）
        name: String,
        /// 选择模块，逗号分隔（不提供则进入交互式选择）
        /// 可选值: sql,cache,session,opendal,scheduler,headless,
        ///         router-common,router-user,router-file,router-model
        #[arg(long, value_delimiter = ',')]
        features: Option<Vec<String>>,
        /// 生成目录，默认为当前目录下的 <name> 子目录
        /// 示例: --output /tmp/projects
        #[arg(long, short = 'o')]
        output: Option<String>,
        /// tibba workspace 本地路径，用于生成 [patch.crates-io] 绕过 crates.io
        /// 示例: --tibba-path ~/github/tibba
        #[arg(long)]
        tibba_path: Option<String>,
    },
}

// ── 入口 ──────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::New { name, features, output, tibba_path } => {
            let selected = match features {
                Some(list) => {
                    let mut set = HashSet::new();
                    for s in &list {
                        match Feature::from_str(s.trim()) {
                            Some(f) => { set.insert(f); }
                            None => {
                                eprintln!("未知模块: {s}");
                                std::process::exit(1);
                            }
                        }
                    }
                    set
                }
                None => interactive_select(),
            };

            let features = resolve(selected);

            let root = match output {
                Some(dir) => expand_tilde(&dir).join(&name),
                None => {
                    let dir = interactive_output();
                    if dir.is_empty() || dir == "." {
                        PathBuf::from(&name)
                    } else {
                        expand_tilde(&dir).join(&name)
                    }
                }
            };

            let tibba_path = match tibba_path {
                Some(p) => Some(expand_tilde(&p)),
                None => interactive_tibba_path(),
            };

            generate_project(&name, &root, &features, tibba_path.as_deref());
        }
    }
}

fn interactive_select() -> HashSet<Feature> {
    let options: Vec<&str> = Feature::selectable().iter().map(|(s, _)| *s).collect();
    let choices = MultiSelect::new("选择需要的模块（空格勾选，回车确认）：", options)
        .prompt()
        .unwrap_or_else(|_| {
            eprintln!("已取消");
            std::process::exit(0);
        });

    let lookup = Feature::selectable();
    let mut set = HashSet::new();
    for choice in choices {
        if let Some((_, f)) = lookup.iter().find(|(s, _)| *s == choice) {
            set.insert(f.clone());
        }
    }
    set
}

fn interactive_output() -> String {
    inquire::Text::new("输出目录（留空则生成到当前目录）：")
        .with_default(".")
        .prompt()
        .unwrap_or_else(|_| {
            eprintln!("已取消");
            std::process::exit(0);
        })
}

/// 询问 tibba workspace 本地路径；留空表示使用 crates.io。
fn interactive_tibba_path() -> Option<PathBuf> {
    let input = inquire::Text::new(
        "tibba 本地路径（留空则从 crates.io 拉取，填路径则生成 [patch.crates-io]）：",
    )
    .with_default("")
    .prompt()
    .unwrap_or_else(|_| {
        eprintln!("已取消");
        std::process::exit(0);
    });

    if input.trim().is_empty() {
        None
    } else {
        Some(expand_tilde(input.trim()))
    }
}

// ── 项目生成 ──────────────────────────────────────────────────────────────────

fn generate_project(name: &str, root: &Path, features: &HashSet<Feature>, tibba_path: Option<&Path>) {
    if root.exists() {
        eprintln!("目录 {} 已存在，请换一个名称或手动删除", root.display());
        std::process::exit(1);
    }

    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("configs")).unwrap();

    write(root, "Cargo.toml", &gen_cargo_toml(name, features, tibba_path));
    write(root, ".env.example", &gen_env_example(features));
    write(root, "configs/default.toml", &gen_default_toml(features));
    write(root, "src/main.rs", &gen_main_rs(features));
    write(root, "src/config.rs", &gen_config_rs(features));
    write(root, "src/state.rs", gen_state_rs());
    write(root, "src/router.rs", &gen_router_rs(features));

    if features.contains(&Feature::Cache) {
        write(root, "src/cache.rs", gen_cache_rs());
    }
    if features.contains(&Feature::Sql) {
        write(root, "src/sql.rs", gen_sql_rs());
    }

    println!("✓ 项目 {name} 已生成: {}", root.display());
    println!("  cd {} && cargo build", root.display());
}

fn write(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    fs::write(&full, content).unwrap_or_else(|e| panic!("写入 {} 失败: {e}", full.display()));
    println!("  生成 {path}");
}

// ── Cargo.toml 生成 ───────────────────────────────────────────────────────────

fn gen_cargo_toml(name: &str, features: &HashSet<Feature>, tibba_path: Option<&Path>) -> String {
    let v = TIBBA_VERSION;

    // 通过模块图计算所有需要的 tibba-* 模块
    let tibba_modules = all_tibba_modules(features);

    let mut out = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8"
tokio = {{ version = "1", features = ["macros", "rt", "rt-multi-thread", "net", "signal"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["local-time"] }}
time = {{ version = "0.3", features = ["serde"] }}
once_cell = "1"
ctor = "0.10"
tower = {{ version = "0.5", features = ["timeout"] }}
tower-http = {{ version = "0.6", features = ["compression-br", "compression-gzip"] }}
rust-embed = "8"
"#
    );

    // 所有 tibba-* 依赖
    for module in &tibba_modules {
        out.push_str(&format!("tibba-{module} = \"{v}\"\n"));
    }

    // 需要额外 feature 的第三方依赖
    if features.contains(&Feature::Sql) {
        out.push_str("sqlx = { version = \"0.8\", features = [\"runtime-tokio\", \"postgres\"] }\n");
    }
    if features.contains(&Feature::Session) {
        out.push_str(
            "axum-extra = { version = \"0.12\", features = [\"cookie\", \"cookie-signed\"] }\n",
        );
    }

    out.push_str(
        r#"
[profile.release]
codegen-units = 1
lto = true
strip = "debuginfo"
"#,
    );

    // 本地路径 patch（绕过 crates.io）
    if let Some(tibba_root) = tibba_path {
        out.push_str("\n[patch.crates-io]\n");
        for module in &tibba_modules {
            let local = tibba_root.join(format!("tibba-{module}"));
            out.push_str(&format!(
                "tibba-{module} = {{ path = \"{}\" }}\n",
                local.display()
            ));
        }
    }

    out
}

// ── .env.example 生成 ─────────────────────────────────────────────────────────

fn gen_env_example(features: &HashSet<Feature>) -> String {
    let mut s = String::from("APP_ENV=development\n");
    if features.contains(&Feature::Sql) {
        s.push_str("DATABASE_URI=postgres://user:password@127.0.0.1:5432/mydb\n");
    }
    if features.contains(&Feature::Cache) {
        s.push_str("REDIS_URI=redis://127.0.0.1:6379\n");
    }
    s
}

// ── configs/default.toml 生成 ─────────────────────────────────────────────────

fn gen_default_toml(features: &HashSet<Feature>) -> String {
    let mut s = String::from(
        r#"[basic]
listen = "0.0.0.0:3000"
processing_limit = 5000
timeout = "30s"
secret = "change-me-to-a-random-secret"
"#,
    );

    if features.contains(&Feature::Sql) {
        s.push_str(
            r#"
[database]
uri = "postgres://user:password@127.0.0.1:5432/mydb"
"#,
        );
    }
    if features.contains(&Feature::Cache) {
        s.push_str(
            r#"
[redis]
uri = "redis://127.0.0.1:6379"
"#,
        );
    }
    if features.contains(&Feature::Session) {
        s.push_str(
            r#"
[session]
ttl = "24h"
secret = "change-me-to-a-64-char-random-secret-for-cookie-signing-xxxxx"
cookie = "sid"
max_renewal = 52
"#,
        );
    }
    s
}

// ── src/state.rs 生成 ────────────────────────────────────────────────────────

fn gen_state_rs() -> &'static str {
    r#"use once_cell::sync::OnceCell;
use tibba_state::AppState;

static APP_STATE: OnceCell<AppState> = OnceCell::new();

pub fn init_app_state(state: AppState) {
    APP_STATE.set(state).expect("app state already initialized");
}

pub fn get_app_state() -> &'static AppState {
    APP_STATE.get().expect("app state not initialized")
}
"#
}

// ── src/config.rs 生成 ───────────────────────────────────────────────────────

fn gen_config_rs(features: &HashSet<Feature>) -> String {
    let session_import = if features.contains(&Feature::Session) {
        "use tibba_session::SessionParams;\nuse axum_extra::extract::cookie::Key;\n"
    } else {
        ""
    };

    let session_config = if features.contains(&Feature::Session) {
        r#"
#[derive(Debug, Clone, Default, Validate, Deserialize)]
pub struct SessionConfig {
    #[serde(with = "tibba_config::humantime_serde")]
    pub ttl: Duration,
    #[validate(length(min = 64))]
    pub secret: String,
    #[validate(length(min = 1, max = 64))]
    pub cookie: String,
    #[serde(default = "default_renewal")]
    pub max_renewal: u8,
}

fn default_renewal() -> u8 { 52 }

static SESSION_CONFIG: OnceCell<SessionConfig> = OnceCell::new();

pub fn get_session_params() -> Result<SessionParams> {
    let cfg = SESSION_CONFIG.get().expect("session config not initialized");
    let key = Key::try_from(cfg.secret.as_bytes()).map_err(map_err)?;
    Ok(SessionParams::new(key)
        .with_cookie(cfg.cookie.clone())
        .with_ttl(cfg.ttl.as_secs() as i64)
        .with_max_renewal(cfg.max_renewal))
}
"#
    } else {
        ""
    };

    let session_init = if features.contains(&Feature::Session) {
        r#"
        let session_cfg: SessionConfig = app_config.sub_config("session").try_deserialize()?;
        session_cfg.validate().map_err(map_err)?;
        SESSION_CONFIG.set(session_cfg).map_err(|_| map_err("session config init failed"))?;"#
    } else {
        ""
    };

    format!(
        r#"use ctor::ctor;
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tibba_config::Config;
use tibba_error::Error;
use tibba_hook::{{BoxFuture, Task, register_task}};
use tibba_util::get_env;
use validator::{{Validate, ValidationError}};
{session_import}
type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(RustEmbed)]
#[folder = "configs/"]
struct Configs;

static CONFIGS: OnceCell<Config> = OnceCell::new();

fn map_err(e: impl ToString) -> Error {{
    Error::new(e).with_category("config")
}}

#[derive(Debug, Clone, Default, Validate, Deserialize)]
pub struct BasicConfig {{
    pub listen: String,
    #[validate(range(min = 0, max = 100000))]
    pub processing_limit: i32,
    #[serde(with = "tibba_config::humantime_serde")]
    pub timeout: Duration,
    pub secret: String,
}}

static BASIC_CONFIG: OnceCell<BasicConfig> = OnceCell::new();

pub fn must_get_basic_config() -> &'static BasicConfig {{
    BASIC_CONFIG.get().expect("basic config not initialized")
}}

pub fn must_get_config() -> &'static Config {{
    CONFIGS.get().expect("config not initialized")
}}
{session_config}
async fn init_config() -> Result<()> {{
    let mut parts = vec![];
    for name in ["default.toml", &format!("{{}}.toml", get_env())] {{
        if let Some(data) = Configs::get(name) {{
            parts.push(String::from_utf8_lossy(&data.data).to_string());
        }}
    }}
    let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    let app_config = Config::new(&refs, Some("APP")).map_err(map_err)?;

    let basic: BasicConfig = app_config.sub_config("basic").try_deserialize().map_err(map_err)?;
    basic.validate().map_err(map_err)?;
    BASIC_CONFIG.set(basic).map_err(|_| map_err("basic config init failed"))?;
    {session_init}
    CONFIGS.set(app_config).map_err(|_| map_err("config init failed"))?;
    Ok(())
}}

struct ConfigTask;
impl Task for ConfigTask {{
    fn before(&self) -> BoxFuture<'_, Result<bool>> {{
        Box::pin(async move {{ init_config().await?; Ok(true) }})
    }}
}}

#[ctor]
fn init() {{
    register_task("config", Arc::new(ConfigTask));
}}
"#
    )
}

// ── src/cache.rs 生成 ────────────────────────────────────────────────────────

fn gen_cache_rs() -> &'static str {
    r#"use crate::config::must_get_config;
use ctor::ctor;
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tibba_cache::{RedisCache, RedisClient, new_redis_client};
use tibba_error::Error;
use tibba_hook::{BoxFuture, Task, register_task};

type Result<T, E = Error> = std::result::Result<T, Error>;
static REDIS_CLIENT: OnceCell<RedisClient> = OnceCell::new();
static REDIS_CACHE: OnceCell<RedisCache> = OnceCell::new();

fn get_redis_client() -> Result<&'static RedisClient> {
    REDIS_CLIENT.get_or_try_init(|| {
        new_redis_client(&must_get_config().sub_config("redis"))
            .map_err(Error::new)
    })
}

pub fn get_redis_cache() -> &'static RedisCache {
    REDIS_CACHE.get_or_init(|| {
        let client = get_redis_client().expect("redis client not initialized");
        RedisCache::new(client)
    })
}

struct RedisTask;
impl Task for RedisTask {
    fn before(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            let _ = get_redis_client()?;
            get_redis_cache().ping().await.map_err(Error::new)?;
            Ok(true)
        })
    }
    fn after(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            if let Ok(client) = get_redis_client() { client.close(); }
            Ok(true)
        })
    }
    fn priority(&self) -> u8 { 16 }
}

#[ctor]
fn init() {
    register_task("redis", Arc::new(RedisTask));
}
"#
}

// ── src/sql.rs 生成 ──────────────────────────────────────────────────────────

fn gen_sql_rs() -> &'static str {
    r#"use crate::config::must_get_config;
use ctor::ctor;
use once_cell::sync::OnceCell;
use sqlx::PgPool;
use std::sync::Arc;
use tibba_error::Error;
use tibba_hook::{BoxFuture, Task, register_task};
use tibba_sql::new_pg_pool;

type Result<T, E = Error> = std::result::Result<T, Error>;
static DB_POOL: OnceCell<PgPool> = OnceCell::new();

pub fn get_db_pool() -> &'static PgPool {
    DB_POOL.get().expect("db pool not initialized")
}

struct SqlTask;
impl Task for SqlTask {
    fn before(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            let pool = new_pg_pool(&must_get_config().sub_config("database"), None)
                .await
                .map_err(Error::new)?;
            DB_POOL.set(pool).map_err(|_| Error::new("set db pool failed"))?;
            Ok(true)
        })
    }
    fn after(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            get_db_pool().close().await;
            Ok(true)
        })
    }
    fn priority(&self) -> u8 { 16 }
}

#[ctor]
fn init() {
    register_task("sql", Arc::new(SqlTask));
}
"#
}

// ── src/router.rs 生成 ───────────────────────────────────────────────────────

fn gen_router_rs(features: &HashSet<Feature>) -> String {
    let mut imports = String::new();
    let mut body = String::from("    let app = axum::Router::new();\n");

    if features.contains(&Feature::RouterCommon) {
        imports.push_str("use tibba_router_common::{CommonRouterParams, new_common_router};\n");
        imports.push_str("use crate::cache::get_redis_cache;\n");
        imports.push_str("use crate::state::get_app_state;\n");
        body.push_str(
            r#"    let app = app.merge(new_common_router(CommonRouterParams {
        state: get_app_state(),
        cache: Some(get_redis_cache()),
    }));
"#,
        );
    }
    if features.contains(&Feature::RouterUser) {
        imports.push_str("use tibba_router_user::{UserRouterParams, new_user_router};\n");
        if !features.contains(&Feature::RouterCommon) {
            imports.push_str("use crate::cache::get_redis_cache;\n");
        }
        imports.push_str("use crate::sql::get_db_pool;\n");
        imports.push_str("use crate::config::must_get_basic_config;\n");
        body.push_str(
            r#"    let app = app.merge(new_user_router(UserRouterParams {
        secret: must_get_basic_config().secret.clone(),
        magic_code: String::new(), // 开发环境验证码魔法码，留空表示不跳过
        pool: get_db_pool(),
        cache: get_redis_cache(),
    }));
"#,
        );
    }
    if features.contains(&Feature::RouterFile) {
        imports.push_str(
            "use tibba_router_file::{FileRouterParams, new_file_router};\nuse tibba_opendal::Storage;\n",
        );
        if !features.contains(&Feature::RouterUser) && !features.contains(&Feature::RouterCommon) {
            imports.push_str("use crate::sql::get_db_pool;\n");
        }
        body.push_str(
            r#"    // TODO: 初始化 Storage（参考 tibba_opendal::new_opendal_storage）
    // let storage: &'static Storage = Box::leak(Box::new(storage));
    // let app = app.merge(new_file_router(FileRouterParams {
    //     storage,
    //     pool: get_db_pool(),
    // }));
"#,
        );
    }
    if features.contains(&Feature::RouterModel) {
        imports.push_str("use tibba_router_model::{ModelRouterParams, new_model_router};\n");
        if !features.contains(&Feature::RouterUser)
            && !features.contains(&Feature::RouterFile)
            && !features.contains(&Feature::RouterCommon)
        {
            imports.push_str("use crate::sql::get_db_pool;\n");
        }
        body.push_str(
            r#"    let app = app.merge(axum::Router::new().nest(
        "/models",
        new_model_router(ModelRouterParams { pool: get_db_pool() }),
    ));
"#,
        );
    }

    body.push_str("    Ok(app)\n");

    format!(
        r#"use tibba_error::Error;
{imports}
pub fn new_router() -> Result<axum::Router, Error> {{
{body}}}
"#
    )
}

// ── src/main.rs 生成 ─────────────────────────────────────────────────────────

fn gen_main_rs(features: &HashSet<Feature>) -> String {
    let mod_cache = if features.contains(&Feature::Cache) { "mod cache;\n" } else { "" };
    let mod_sql = if features.contains(&Feature::Sql) { "mod sql;\n" } else { "" };

    let scheduler_run = if features.contains(&Feature::Scheduler) {
        "    tibba_scheduler::run_scheduler_jobs().await.expect(\"scheduler start failed\");\n"
    } else {
        ""
    };

    let session_layer = if features.contains(&Feature::Session) {
        r#"
    use axum::middleware::from_fn_with_state;
    use tibba_session::session as session_middleware;
    use std::sync::Arc;
    let session_params = Arc::new(config::get_session_params().expect("session params failed"));
    let cache = cache::get_redis_cache();
"#
    } else {
        ""
    };

    let session_layer_apply = if features.contains(&Feature::Session) {
        "            .layer(from_fn_with_state((cache, session_params), session_middleware))\n"
    } else {
        ""
    };

    format!(
        r#"mod config;
mod router;
mod state;
{mod_cache}{mod_sql}
use axum::BoxError;
use axum::error_handling::HandleErrorLayer;
use axum::http::{{Method, Uri}};
use axum::middleware::from_fn_with_state;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tibba_hook::{{run_after_tasks, run_before_tasks}};
use tibba_middleware::{{entry, processing_limit, stats}};
use tibba_state::AppState;
use tibba_util::is_development;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tracing::{{Level, error, info}};
use tracing_subscriber::FmtSubscriber;

pub async fn handle_error(method: Method, uri: Uri, err: BoxError) -> tibba_error::Error {{
    error!(method = %method, uri = %uri, err = %err);
    if err.is::<tower::timeout::error::Elapsed>() {{
        tibba_error::Error::new("request took too long").with_status(408)
    }} else {{
        tibba_error::Error::new(err.to_string()).with_status(500)
    }}
}}

async fn shutdown_signal() {{
    let ctrl_c = async {{ signal::ctrl_c().await.expect("install Ctrl+C handler failed") }};
    #[cfg(unix)]
    let terminate = async {{
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install signal handler failed")
            .recv()
            .await;
    }};
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {{
        _ = ctrl_c => {{}},
        _ = terminate => {{}},
    }}
    info!("signal received, starting graceful shutdown");
}}

fn init_logger() {{
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| Level::from_str(&v).ok())
        .unwrap_or(Level::INFO);
    let timer = tracing_subscriber::fmt::time::OffsetTime::local_rfc_3339()
        .unwrap_or_else(|_| tracing_subscriber::fmt::time::OffsetTime::new(
            time::UtcOffset::UTC,
            time::format_description::well_known::Rfc3339,
        ));
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_timer(timer)
        .with_ansi(is_development())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("set global subscriber failed");
}}

async fn run() -> Result<(), Box<dyn std::error::Error>> {{
    run_before_tasks().await?;
{scheduler_run}
    let basic = config::must_get_basic_config();
    let app_state = AppState::new(basic.processing_limit, "--");
    state::init_app_state(app_state);
    let state = state::get_app_state();
    state.run();
{session_layer}
    let app = router::new_router()?;
    let app = app.layer(
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(handle_error))
            .timeout(basic.timeout)
            .layer(from_fn_with_state(state, entry))
            .layer(from_fn_with_state(state, stats))
{session_layer_apply}            .layer(from_fn_with_state(state, processing_limit)),
    );

    info!("listening on http://{{}}/", basic.listen);
    let listener = tokio::net::TcpListener::bind(&basic.listen).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}}

async fn start() {{
    if let Err(e) = run().await {{
        error!(category = "launch_app", message = ?e);
    }}
    if let Err(e) = run_after_tasks().await {{
        error!(category = "run_after_tasks", message = ?e);
    }}
}}

fn main() {{
    init_logger();
    let cpus = std::env::var("THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
    info!(threads = cpus, "starting server");
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(cpus)
        .build()
        .expect("build tokio runtime failed")
        .block_on(start());
}}
"#
    )
}
