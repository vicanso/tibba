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
//! 启动期显式调用 [`install_recorder`] 装载全局 recorder，`/metrics` 端点由
//! [`metrics_handler`] 输出当前指标文本快照。

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;
use tibba_error::Error;

type Result<T> = std::result::Result<T, Error>;

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

fn map_err(err: impl ToString) -> Error {
    Error::new(err).with_category("metrics")
}

/// 安装全局 Prometheus recorder 并保存 handle，须在中间件与后台任务启动前调用。
pub fn install_recorder() -> Result<()> {
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(map_err)?;
    HANDLE
        .set(handle)
        .map_err(|_| map_err("prometheus recorder already installed"))?;
    Ok(())
}

/// Axum 处理器：返回当前指标快照（Prometheus text exposition 格式）。
pub async fn metrics_handler() -> String {
    HANDLE
        .get()
        .unwrap_or_else(|| panic!("prometheus recorder not initialized"))
        .render()
}
