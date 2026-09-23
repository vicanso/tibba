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

use crate::router::new_router;
use axum::BoxError;
use axum::error_handling::HandleErrorLayer;
use axum::extract::DefaultBodyLimit;
use axum::http::{Method, Uri};
use axum::middleware::{from_fn, from_fn_with_state};
use opentelemetry_otlp::WithExportConfig;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tibba_middleware::{
    Cors, HttpCache, MiddlewareOptions, SecurityHeaders, cors, entry, http_cache, otel_trace,
    processing_limit, request_id, security_headers, stats, validate_csrf,
};
use tibba_router_user::api_key_auth;
use tibba_runtime::{
    install_shutdown_signal, run_after_tasks, run_before_tasks, run_scheduler_jobs, shutdown_token,
};
use tibba_session::session;
use tibba_util::{is_development, is_production};
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 应用入口模块的 tracing target。
/// 可通过 `RUST_LOG=tibba:app=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:app";

/// 进程退出时等待异步任务队列排空的上限：HTTP 优雅关闭后通知 worker 跑完在途任务，
/// 超过此时长则放弃等待，残留在途任务由 reaper 在可见性超时后重排。
const JOB_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 全局请求体上限（2 MiB）：保护 JSON / 表单等缓冲型端点免受超大请求体 DoS。
/// 文件上传（multipart 流式）在路由层用 `DefaultBodyLimit::disable()` 豁免此限，
/// 改由 file 路由内部按其 `MAX_UPLOAD_BYTES` 逐块校验。
const GLOBAL_BODY_LIMIT: usize = 2 * 1024 * 1024;

mod admin_web;
mod app_ctx;
mod cache;
mod config;
mod dal;
mod feature;
mod i18n;
mod job;
mod metrics;
mod openapi;
mod router;
mod sql;
mod state;
mod user_admin;

#[cfg(feature = "demo-docker")]
mod docker;
#[cfg(feature = "demo-docker")]
mod model;

#[cfg(feature = "demo-detector")]
mod httpstat;

#[cfg(feature = "demo-token")]
mod llm;

#[cfg(feature = "demo-tenant")]
mod tenant;

// Global error handler for the application
// Processes unhandled errors and converts them into appropriate Error responses
// Handles special cases like timeout errors
pub async fn handle_error(
    method: Method, // HTTP method of the request
    uri: Uri,       // URI of the request
    err: BoxError,  // The error that occurred
) -> tibba_error::Error {
    // Log the error with request details
    error!(method = method.to_string(), uri = uri.to_string(), err,);
    // error!("method:{}, uri:{}, error:{}", method, uri, err.to_string());

    // Special handling for timeout errors
    // Otherwise treats as internal server error (500)
    let (message, category, status) = if err.is::<tower::timeout::error::Elapsed>() {
        (
            "Request took too long".to_string(),
            "timeout".to_string(),
            408,
        )
    } else {
        (err.to_string(), "exception".to_string(), 500)
    };

    tibba_error::Error::new(message)
        .with_category(category)
        .with_status(status)
}

/// 初始化日志 + 可选 OTLP 分布式追踪。
///
/// OTel 配置走标准环境变量（与 OTel SDK 约定一致）：
/// - `OTEL_EXPORTER_OTLP_ENDPOINT` —— OTLP collector 地址，如 `http://otel-collector:4318`。
///   缺省 / 空时 telemetry 完全关闭，仅打 fmt 日志，0 网络开销
/// - `OTEL_SERVICE_NAME` —— 资源 `service.name` 标签；缺省 `tibba`
///
/// ## 日志级别：`RUST_LOG` 支持**按 target 过滤**
///
/// 各 `tibba-*` crate 都给自己的日志打了 target（`tibba:cache` / `tibba:hook`
/// …），`RUST_LOG` 因此可以精确到模块：
///
/// ```text
/// RUST_LOG=info                      # 全局 info
/// RUST_LOG=info,tibba:cache=debug    # 只把缓存模块调到 debug
/// RUST_LOG=warn,tibba:request=info   # 只看出站请求
/// ```
///
/// 此前这里用的是 `Level::from_str(RUST_LOG)` + `LevelFilter`：只认单个级别名，
/// `tibba:cache=debug` 这种写法解析失败后**静默**回退到全局 info——各 crate 文档
/// 里承诺的 target 过滤其实一直不可用，唯一的调节手段是把全局级别整体抬高或
/// 压低。换成 `EnvFilter` 后那套约定才真正生效。
///
/// 解析失败（写错指令）时回退到 `info`，不让日志配置写错把进程拦在启动前。
fn init_logger() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let timer = tracing_subscriber::fmt::time::OffsetTime::local_rfc_3339().unwrap_or_else(|_| {
        tracing_subscriber::fmt::time::OffsetTime::new(
            time::UtcOffset::UTC,
            time::format_description::well_known::Rfc3339,
        )
    });

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_timer(timer)
        .with_ansi(is_development());

    // OTLP layer 由环境变量决定是否启用；endpoint 为空 → 直接 None，零开销
    let otel_layer = build_otel_layer();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();
}

/// 进程级 OTLP provider 句柄：优雅退出时 `shutdown` 刷出残余 span。
static OTEL_PROVIDER: std::sync::OnceLock<opentelemetry_sdk::trace::SdkTracerProvider> =
    std::sync::OnceLock::new();

/// 构造 OTLP tracing layer。配置缺失 / 构造失败时返回 None，
/// 让上游的 `.with(None)` 静默忽略——与"未启用"语义等价。
///
/// 用 HTTP/Protobuf 传输（最通用、最少 SDK 依赖）。
fn build_otel_layer<S>()
-> Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    let endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty())?;
    let service_name = env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "tibba".to_string());

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(&endpoint)
        .build()
        .map_err(|e| eprintln!("otlp exporter init failed (telemetry disabled): {e}"))
        .ok()?;

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name(service_name)
                .build(),
        )
        .build();
    let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, "tibba");

    // 克隆一份挂全局，原件存 OnceLock 供进程退出时 shutdown
    opentelemetry::global::set_tracer_provider(provider.clone());
    let _ = OTEL_PROVIDER.set(provider);
    // 注册 W3C Trace Context 全局 propagator：入站 otel_trace 中间件据此提取上游
    // traceparent，出站 tibba-request 据此注入，二者共用同一传播格式串起跨服务调用链。
    // 仅在 OTLP 启用时设置，关闭时沿用默认 no-op propagator，提取 / 注入均零开销。
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    Some(tracing_opentelemetry::layer().with_tracer(tracer))
}

/// 优雅关闭 OTLP：刷出 batch 中未导出的 span。未启用 telemetry 时为 no-op。
fn shutdown_otel() {
    if let Some(provider) = OTEL_PROVIDER.get() {
        if let Err(e) = provider.shutdown() {
            warn!(target: LOG_TARGET, error = %e, "otel provider shutdown failed");
        } else {
            info!(target: LOG_TARGET, "otel provider shut down");
        }
    }
}

/// 按 `basic.super_admins` 为已存在的账号授予超级管理员角色（幂等）。
///
/// 超管的来源必须是运维显式声明的配置，而不是「谁先注册谁就是」。账号尚不存在
/// 时仅告警：先注册，再重启即可生效。
async fn bootstrap_super_admins(
    pool: &'static sqlx::PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    use tibba_model::Model;
    let model = tibba_model::UserModel::new();
    for account in &config::must_get_basic_config().super_admins {
        let granted = model
            .grant_role_by_account(pool, account, tibba_model::ROLE_SUPER_ADMIN)
            .await
            .map_err(tibba_error::Error::from)?;
        if granted {
            warn!(target: LOG_TARGET, account, "granted super admin role");
        } else if model
            .get_by_account(pool, account)
            .await
            .map_err(tibba_error::Error::from)?
            .is_none()
        {
            warn!(target: LOG_TARGET, account, "super admin account not found, register it then restart");
        }
    }
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // 尽早安装：SIGINT / SIGTERM 都触发同一个进程级信号，HTTP 服务、cron 调度器
    // 与各后台循环共享它。放在 before hooks 之前，使启动阶段收到的信号也不会丢。
    install_shutdown_signal();
    // 启动第一行就把运行模式打出来：dev / test 会放宽 secret、验证码、cookie 等
    // 安全要求，运维需要一眼确认线上没有误跑在开发模式。
    info!(
        target: LOG_TARGET,
        rust_env = tibba_util::get_env(),
        production = tibba_util::is_production(),
        "starting"
    );
    if !tibba_util::is_production() {
        warn!(
            target: LOG_TARGET,
            rust_env = tibba_util::get_env(),
            "running with development relaxations (demo secret, captcha bypass, insecure cookies); never use in production"
        );
    }
    run_before_tasks().await?;
    // before hooks 已就绪 DB/Redis/OpenDAL/AppState → 组装显式 DI 容器
    let ctx = app_ctx::AppCtx::install_from_globals()?;
    bootstrap_super_admins(ctx.pool).await?;
    // JWT access token 的撤销校验与 Session 共用 Redis 中的撤销标记
    tibba_jwt::init_revocation_cache(ctx.cache);
    run_scheduler_jobs().await?;

    // 注册异步任务 handler 并启动 worker（DB 池已在 run_before_tasks 中初始化）。
    // 并发 worker 数 = 同时执行的任务上限，按需调整。
    job::register_job_handlers()?;
    // 注册 i18n 本地化目录（按 Accept-Language 本地化错误响应 message）
    i18n::init();
    // 持有句柄以便进程退出前优雅排空在途任务（见下方 serve 之后的 shutdown）
    let job_workers = tibba_job::start(ctx.pool, 4);

    let basic_config = config::must_get_basic_config();
    let app = new_router(ctx)?;

    // 可选中间件开关（默认全开）；最小服务可用 MiddlewareOptions::minimal() 裁剪
    let mw = MiddlewareOptions::default();
    info!(
        target: LOG_TARGET,
        otel = mw.otel,
        http_cache = mw.http_cache,
        i18n = mw.i18n,
        api_key = mw.api_key,
        csrf = mw.csrf,
        "middleware options"
    );

    // CORS：配置白名单 + 生产 fail-fast（禁止任意来源）
    let mut cors_cfg = Cors::default().with_allow_credentials(basic_config.cors_allow_credentials);
    for origin in &basic_config.cors_allow_origins {
        cors_cfg = cors_cfg.add_allow_origin(origin);
    }
    if is_production() {
        cors_cfg
            .assert_production_safe()
            .map_err(|msg| -> Box<dyn std::error::Error> { msg.into() })?;
    }

    let predicate = SizeAbove::new(1024)
        .and(NotForContentType::GRPC)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE);
    let state = ctx.state;
    // 包成 Arc 以便 session 与 api_key_auth 两个中间件共享同一份配置
    let session_params = Arc::new(config::get_session_params()?);

    // 外层固定：错误处理 / 压缩 / 超时 / body limit / request_id /（可选）otel
    // Tower ServiceBuilder 按添加顺序包在请求路径外侧。
    let mut app = app.layer(
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(handle_error))
            .layer(CompressionLayer::new().compress_when(predicate))
            .timeout(basic_config.timeout)
            .layer(DefaultBodyLimit::max(GLOBAL_BODY_LIMIT))
            .layer(from_fn(request_id)),
    );
    if mw.otel {
        // 无 OTLP endpoint 时 otel_trace 内部 no-op；开关关掉则整层不挂
        app = app.layer(from_fn(otel_trace));
    }

    // 安全头 + CORS 始终开启（CORS 策略由配置收敛）
    app = app
        .layer(from_fn_with_state(
            SecurityHeaders::default().with_content_security_policy(
                "object-src 'none'; frame-ancestors 'none'; base-uri 'self'",
            ),
            security_headers,
        ))
        .layer(from_fn_with_state(Arc::new(cors_cfg), cors));

    if mw.http_cache {
        app = app.layer(from_fn_with_state(HttpCache::default(), http_cache));
    }
    if mw.i18n {
        app = app.layer(from_fn(tibba_i18n::i18n));
    }
    app = app
        .layer(from_fn_with_state(state, entry))
        .layer(from_fn_with_state(state, stats))
        .layer(from_fn_with_state(
            (ctx.cache, session_params.clone()),
            session,
        ));
    if mw.api_key {
        // API Key：紧随 session。有效令牌注入登录 Session；无效则按未登录放行
        app = app.layer(from_fn_with_state(
            (ctx.pool, ctx.cache, session_params.clone()),
            api_key_auth,
        ));
    }
    if mw.csrf {
        app = app.layer(from_fn(validate_csrf));
    }
    let app = app.layer(from_fn_with_state(state, processing_limit));
    state.run();

    info!("listening on http://{}/", basic_config.listen);
    let listener = tokio::net::TcpListener::bind(basic_config.listen.clone()).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_token().cancelled_owned())
    .await?;

    // HTTP 已优雅排空（不再有新请求入队任务），再排空在途异步任务：
    // 通知 worker 停止认领、跑完手头任务后退出，最多等 JOB_DRAIN_TIMEOUT
    let pool = ctx.pool;
    if let Ok(before) = tibba_job::JobQueue::new(pool).stats().await {
        info!(
            target: LOG_TARGET,
            pending = before.pending,
            running = before.running,
            "draining job workers before exit"
        );
    }
    job_workers.shutdown(JOB_DRAIN_TIMEOUT).await;
    // 排空后再采一次：便于运维判断 JOB_DRAIN_TIMEOUT 是否够；残留在途任务将由 reaper 重排
    if let Ok(after) = tibba_job::JobQueue::new(pool).stats().await {
        if after.running > 0 {
            warn!(
                target: LOG_TARGET,
                pending = after.pending,
                running = after.running,
                "job workers drained with in-flight tasks remaining; reaper will requeue them"
            );
        } else {
            info!(
                target: LOG_TARGET,
                pending = after.pending,
                "job workers drained cleanly"
            );
        }
    }
    // 在进程退出前刷出 OTLP 残余 span，避免丢最后一批请求追踪
    shutdown_otel();
    Ok(())
}

async fn start() {
    // only use unwrap in run function
    if let Err(e) = run().await {
        error!(target: LOG_TARGET, event = "launch_app", message = ?e)
    }
    if let Err(e) = run_after_tasks().await {
        error!(target: LOG_TARGET, event = "run_after_tasks", message = ?e);
    }
}

/// 进程 panic 时的 best-effort 企业微信告警。
///
/// - 未设置 / 空 `TIBBA_PANIC_WECOM_KEY` → 静默跳过
/// - 发送失败只写 stderr，不再 panic
fn alert_panic_best_effort(message: &str) {
    let Ok(key) = env::var("TIBBA_PANIC_WECOM_KEY") else {
        return;
    };
    if key.is_empty() {
        return;
    }
    let message = message.to_string();
    // join 带超时：告警最多挡退出 ~3s，避免 hook 挂死
    let handle = std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("panic alert: runtime build failed: {e}");
                return;
            }
        };
        rt.block_on(async move {
            let notifier = match tibba_notify::WecomRobotNotifier::new() {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("panic alert: wecom init failed: {e}");
                    return;
                }
            };
            use tibba_notify::{Notifier, NotifyMessage};
            let msg = NotifyMessage::new("tibba panic", message);
            if let Err(e) = notifier.send(&key, &msg).await {
                eprintln!("panic alert: send failed: {e}");
            }
        });
    });
    let _ = handle.join();
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        error!(target: LOG_TARGET, event = "panic", message = %message);
        // best-effort 告警：环境变量 TIBBA_PANIC_WECOM_KEY = 企业微信机器人 key。
        // 独立线程 + current_thread runtime，避免在 hook 里依赖可能已崩的全局 runtime。
        alert_panic_best_effort(&message);
        std::process::exit(1);
    }));
    init_logger();
    let cpus = std::env::var("TIBBA_THREADS")
        .map(|v| v.parse::<usize>().unwrap_or(num_cpus::get()))
        .unwrap_or(num_cpus::get())
        .max(1);
    info!(threads = cpus, "start static server");
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(cpus)
        .build()
        .unwrap_or_else(|e| panic!("failed to build tokio runtime: {}", e))
        .block_on(start());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;
    use tracing_subscriber::filter::LevelFilter;

    /// **回归守卫**：`RUST_LOG` 必须支持按 target 过滤。
    ///
    /// 各 `tibba-*` crate 的文档都在承诺 `RUST_LOG=tibba:cache=debug` 可用，
    /// 而这只有在订阅器装的是 `EnvFilter` 时才成立。此前装的是 `LevelFilter`，
    /// 这类指令解析失败后静默回退全局 info——承诺一直是空的。
    ///
    /// target 里带 `:` 是本项目的命名约定（`tibba:cache`），这里一并钉住
    /// EnvFilter 确实接受这种写法。
    #[test]
    fn rust_log_supports_per_target_directives() {
        for spec in [
            "info",
            // 旧写法（含大写）必须继续可用，否则升级会静默改变现网日志级别
            "INFO",
            "Debug",
            "warn",
            "tibba:cache=debug",
            "info,tibba:cache=debug",
            "warn,tibba:request=info,tibba:hook=trace",
        ] {
            assert!(
                EnvFilter::try_new(spec).is_ok(),
                "RUST_LOG={spec:?} 应当可被解析"
            );
        }
    }

    /// 按 target 抬高级别时，整体的 max level 必须跟着抬高，
    /// 否则事件会在更外层就被静态过滤掉，指令等于没写。
    #[test]
    fn per_target_directive_raises_max_level() {
        let filter = EnvFilter::try_new("warn,tibba:cache=debug").expect("指令应可解析");
        assert_eq!(filter.max_level_hint(), Some(LevelFilter::DEBUG));

        // 未抬高时不应误报
        let filter = EnvFilter::try_new("warn").expect("指令应可解析");
        assert_eq!(filter.max_level_hint(), Some(LevelFilter::WARN));
    }

    /// 写错的指令不能把进程拦在启动前，回退到 info。
    #[test]
    fn malformed_directive_falls_back_to_info() {
        assert!(EnvFilter::try_new("=not a level=").is_err());
        // init_logger 的回退路径
        let filter = EnvFilter::try_new("=not a level=")
            .unwrap_or_else(|_| EnvFilter::new(Level::INFO.as_str()));
        assert_eq!(filter.max_level_hint(), Some(LevelFilter::INFO));
    }
}
