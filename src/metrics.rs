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

//! Prometheus 指标接入。
//!
//! 中间件通过 `metrics::counter!` / `gauge!` 宏写入指标，由全局
//! Prometheus recorder 收集；本模块负责启动期装载 recorder，并提供
//! `/metrics` 端点把当前快照按 Prometheus text exposition 格式输出。

use ctor::ctor;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tibba_error::Error;
use tibba_hook::{BoxFuture, Task, register_task};

type Result<T> = std::result::Result<T, Error>;

static HANDLE: OnceCell<PrometheusHandle> = OnceCell::new();

fn map_err(err: impl ToString) -> Error {
    Error::new(err).with_category("metrics")
}

/// 安装全局 Prometheus recorder，并保存 PrometheusHandle 以便 `/metrics`
/// 端点渲染快照。重复调用会返回错误。
fn install_recorder() -> Result<()> {
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(map_err)?;
    HANDLE
        .set(handle)
        .map_err(|_| map_err("prometheus recorder already installed"))?;
    Ok(())
}

/// 取出已安装的 PrometheusHandle；必须在 MetricsTask::before 之后调用。
fn get_metrics_handle() -> &'static PrometheusHandle {
    HANDLE
        .get()
        .unwrap_or_else(|| panic!("prometheus recorder not initialized"))
}

/// Axum 处理器：返回当前指标快照（Prometheus text exposition 格式）。
pub async fn metrics_handler() -> String {
    get_metrics_handle().render()
}

struct MetricsTask;

impl Task for MetricsTask {
    fn before(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            install_recorder()?;
            Ok(true)
        })
    }
    // 与 StateTask 一致取最高优先级，确保中间件首次触发 counter!/gauge! 之前
    // recorder 已就位（未安装 recorder 时宏会落到 noop，丢失早期请求计数）
    fn priority(&self) -> u8 {
        u8::MAX
    }
}

#[ctor(unsafe)]
fn init() {
    register_task("metrics", Arc::new(MetricsTask));
}
