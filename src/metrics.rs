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

//! Prometheus 指标接入与 `/metrics` 端点访问控制。
//!
//! 启动期显式调用 [`install_recorder`] 装载全局 recorder，`/metrics` 端点由
//! [`metrics_handler`] 校验 Bearer Token（`basic.metrics_token`）或 `AdminSession` 后输出当前指标文本快照。

use crate::config::must_get_basic_config;
use axum::http::HeaderMap;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;
use subtle::ConstantTimeEq;
use tibba_auth::Session;
use tibba_error::Error;
use tibba_util::{is_development, is_test};

type Result<T> = std::result::Result<T, Error>;

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

fn map_err(err: impl ToString) -> Error {
    Error::new(err).with_category("metrics")
}

/// 判定请求是否有权读取 `/metrics`：
/// - `AdminSession` 始终放行
/// - 配置了 `metrics_token` 时要求 `Authorization: Bearer <token>` 常量时间匹配
/// - 未配置 `metrics_token` 时仅 dev/test 环境放行，生产环境拒绝匿名与普通用户
pub(crate) fn check_metrics_access(
    configured_token: Option<&str>,
    auth_header: Option<&str>,
    is_admin_session: bool,
    is_dev_or_test: bool,
) -> bool {
    if is_admin_session {
        return true;
    }
    let expected = configured_token.map(str::trim).filter(|s| !s.is_empty());
    match expected {
        Some(secret) => {
            let Some(header) = auth_header.map(str::trim) else {
                return false;
            };
            let token = header
                .strip_prefix("Bearer ")
                .or_else(|| header.strip_prefix("bearer "))
                .map(str::trim);
            match token {
                Some(candidate) => {
                    let a = candidate.as_bytes();
                    let b = secret.as_bytes();
                    a.len() == b.len() && bool::from(a.ct_eq(b))
                }
                None => false,
            }
        }
        None => is_dev_or_test,
    }
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

/// Axum 处理器：校验访问权限后返回当前指标快照（Prometheus text exposition 格式）。
pub async fn metrics_handler(headers: HeaderMap, session: Session) -> Result<String> {
    let basic = must_get_basic_config();
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    let allowed = check_metrics_access(
        basic.metrics_token.as_deref(),
        auth_header,
        session.is_admin(),
        is_development() || is_test(),
    );
    if !allowed {
        return Err(Error::new("metrics access unauthorized")
            .with_category("metrics")
            .with_sub_category("metrics_unauthorized")
            .with_status(401)
            .with_exception(false));
    }

    Ok(HANDLE
        .get()
        .unwrap_or_else(|| panic!("prometheus recorder not initialized"))
        .render())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_access_control_enforces_token_and_admin_session() {
        // 已配置 token：匹配 Bearer 或 AdminSession 放行；空头/错 token/长度不符拒绝
        assert!(check_metrics_access(
            Some("secret-token-123"),
            Some("Bearer secret-token-123"),
            false,
            false
        ));
        assert!(check_metrics_access(
            Some("secret-token-123"),
            Some("bearer secret-token-123"),
            false,
            true
        ));
        assert!(check_metrics_access(
            Some("secret-token-123"),
            None,
            true,
            false
        ));
        assert!(!check_metrics_access(
            Some("secret-token-123"),
            None,
            false,
            true
        ));
        assert!(!check_metrics_access(
            Some("secret-token-123"),
            Some("Basic c2VjcmV0"),
            false,
            true
        ));
        assert!(!check_metrics_access(
            Some("secret-token-123"),
            Some("Bearer wrong"),
            false,
            false
        ));
        assert!(!check_metrics_access(
            Some("secret-token-123"),
            Some("Bearer secret-token-124"),
            false,
            false
        ));

        // 未配置 token（或仅空白）：仅 dev/test 与 AdminSession 放行，生产非管理员拒绝
        assert!(check_metrics_access(None, None, true, false));
        assert!(check_metrics_access(None, None, false, true));
        assert!(!check_metrics_access(None, None, false, false));
        assert!(!check_metrics_access(Some("   "), None, false, false));
    }
}
