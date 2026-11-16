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

#[cfg(feature = "otel")]
use opentelemetry_otlp::WithExportConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tibba_middleware::{Cors, MiddlewareOptions, MiddlewareStackParams, apply_middleware_stack};
use tibba_runtime::{install_shutdown_signal, shutdown_token};
use tibba_util::{is_development, is_production};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 应用入口模块的 tracing target。
/// 可通过 `RUST_LOG=tibba:app=info`（或 `debug`）进行过滤。
const LOG_TARGET: &str = "tibba:app";

/// 进程退出时等待异步任务队列排空的上限：HTTP 优雅关闭后通知 worker 跑完在途任务，
/// 超过此时长则放弃等待，残留在途任务由 reaper 在可见性超时后重排。
#[cfg(feature = "job")]
const JOB_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

mod admin_web;
mod app_ctx;
mod config;
mod i18n;
mod metrics;
mod openapi;
mod router;

/// 初始化日志 + 可选 OTLP 分布式追踪。
///
/// OTel 配置走标准环境变量（与 OTel SDK 约定一致）：
/// - `OTEL_EXPORTER_OTLP_ENDPOINT` —— OTLP collector 地址，如 `http://otel-collector:4318`。
///   缺省 / 空时 telemetry 完全关闭，仅打 fmt 日志，0 网络开销
/// - `OTEL_SERVICE_NAME` —— 资源 `service.name` 标签；缺省 `tibba`
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

    #[cfg(feature = "otel")]
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(build_otel_layer());

    #[cfg(not(feature = "otel"))]
    let registry = tracing_subscriber::registry().with(filter).with(fmt_layer);

    registry.init();
}

/// 进程级 OTLP provider 句柄：优雅退出时 `shutdown` 刷出残余 span。
#[cfg(feature = "otel")]
static OTEL_PROVIDER: std::sync::OnceLock<opentelemetry_sdk::trace::SdkTracerProvider> =
    std::sync::OnceLock::new();

/// 构造 OTLP tracing layer。配置缺失 / 构造失败时返回 None，
/// 让上游的 `.with(None)` 静默忽略——与"未启用"语义等价。
#[cfg(feature = "otel")]
fn build_otel_layer<S>()
-> Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty())?;
    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "tibba".to_string());

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
    // 注册 W3C Trace Context 全局 propagator：入站 otel_trace 与出站 tibba-request 共用
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    Some(tracing_opentelemetry::layer().with_tracer(tracer))
}

/// 优雅关闭 OTLP：刷出 batch 中未导出的 span。未启用 telemetry 时为 no-op。
#[cfg(feature = "otel")]
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
    use tibba_model::{Model, ROLE_SUPER_ADMIN, UserModel};
    let model = UserModel::new();
    for account in &config::must_get_basic_config().super_admins {
        let granted = model
            .grant_role_by_account(pool, account, ROLE_SUPER_ADMIN)
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
            warn!(
                target: LOG_TARGET,
                account,
                "super admin account not found, register it then restart"
            );
        }
    }
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // 尽早安装：SIGINT / SIGTERM 都触发同一个进程级信号，HTTP 服务、cron 调度器
    // 与各后台循环共享它。
    install_shutdown_signal();
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
            "running with development relaxations (demo secret, insecure cookies); never use in production"
        );
    }

    config::init_config()?;
    metrics::install_recorder()?;
    i18n::init();

    let ctx = app_ctx::init().await?;
    bootstrap_super_admins(ctx.pool).await?;
    tibba_router::rbac::ensure_builtin_permissions(ctx.pool).await?;

    // JWT access token 的撤销校验与 Session 共用 Redis 中的撤销标记
    #[cfg(feature = "jwt")]
    {
        tibba_auth::jwt::init_revocation_cache(ctx.cache);
    }

    // 持有句柄以便进程退出前优雅排空在途任务（见下方 serve 之后的 shutdown）
    #[cfg(feature = "job")]
    let job_workers = tibba_model::job::start(ctx.pool, 4);

    let basic_config = config::must_get_basic_config();
    let app = router::new_router(ctx)?;

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

    let mw = MiddlewareOptions::default()
        .with_otel(cfg!(feature = "otel"))
        .with_api_key(cfg!(feature = "api-key") || cfg!(feature = "app-auth"));
    info!(
        target: LOG_TARGET,
        otel = mw.otel,
        http_cache = mw.http_cache,
        i18n = mw.i18n,
        api_key = mw.api_key,
        csrf = mw.csrf,
        "middleware options"
    );

    let session_params = Arc::new(config::get_session_params()?);
    #[cfg(feature = "api-key")]
    let api_session_params = session_params.clone();
    #[allow(unused_mut)]
    let mut stack_params = MiddlewareStackParams::new(ctx.state, ctx.cache, session_params)
        .with_cors(cors_cfg)
        .with_timeout(basic_config.timeout)
        .with_options(mw);
    #[cfg(feature = "api-key")]
    {
        let pool = ctx.pool;
        let cache = ctx.cache;
        stack_params = stack_params.with_post_session_layer(move |router| {
            router.layer(axum::middleware::from_fn_with_state(
                (pool, cache, api_session_params),
                tibba_router::user::api_key_auth,
            ))
        });
    }
    #[cfg(feature = "app-auth")]
    {
        let app_auth_state = tibba_router::app_auth::new_app_auth_state(
            ctx.configs,
            ctx.cache,
            &basic_config.secret,
        );
        stack_params = stack_params.with_post_session_layer(move |router| {
            router.layer(axum::middleware::from_fn_with_state(
                app_auth_state,
                tibba_router::app_auth::app_auth_middleware,
            ))
        });
    }
    let app = apply_middleware_stack(app, stack_params);

    let state = ctx.state;
    state.run();

    let bind_addr = basic_config.bind_addr();
    info!("listening on http://{bind_addr}/");
    let listener = tokio::net::TcpListener::bind(&*bind_addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_token().cancelled().await;
        // 收到退出信号第一时间切换为停止态，让 /healthz / /readyz 尽快摘除实例
        state.stop();
        if is_production() {
            // 给负载均衡预留摘流窗口，再停止接新连接并排空在途 HTTP 请求
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    })
    .await?;

    // HTTP 已优雅排空（不再有新请求入队任务），再排空在途异步任务
    #[cfg(feature = "job")]
    {
        let pool = ctx.pool;
        if let Ok(before) = tibba_model::job::JobQueue::new(pool).stats().await {
            info!(
                target: LOG_TARGET,
                pending = before.pending,
                running = before.running,
                "draining job workers before exit"
            );
        }
        job_workers.shutdown(JOB_DRAIN_TIMEOUT).await;
        if let Ok(after) = tibba_model::job::JobQueue::new(pool).stats().await {
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
    }

    app_ctx::shutdown(ctx).await;

    #[cfg(feature = "otel")]
    shutdown_otel();

    Ok(())
}

async fn start() {
    if let Err(e) = run().await {
        error!(target: LOG_TARGET, event = "launch_app", message = ?e);
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        error!(target: LOG_TARGET, event = "panic", message = %message);
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

    #[test]
    fn rust_log_supports_per_target_directives() {
        for spec in [
            "info",
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

    #[test]
    fn per_target_directive_raises_max_level() {
        let filter = EnvFilter::try_new("warn,tibba:cache=debug").expect("指令应可解析");
        assert_eq!(filter.max_level_hint(), Some(LevelFilter::DEBUG));

        let filter = EnvFilter::try_new("warn").expect("指令应可解析");
        assert_eq!(filter.max_level_hint(), Some(LevelFilter::WARN));
    }

    #[test]
    fn malformed_directive_falls_back_to_info() {
        assert!(EnvFilter::try_new("=not a level=").is_err());
        let filter = EnvFilter::try_new("=not a level=")
            .unwrap_or_else(|_| EnvFilter::new(Level::INFO.as_str()));
        assert_eq!(filter.max_level_hint(), Some(LevelFilter::INFO));
    }
}
