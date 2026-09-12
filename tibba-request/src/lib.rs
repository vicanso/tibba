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

use snafu::Snafu;
use tibba_error::Error as BaseError;

/// 该 crate 所有日志事件的 tracing target。
/// 可通过 `RUST_LOG=tibba:request=info`（或 `debug`）进行过滤。
pub(crate) const LOG_TARGET: &str = "tibba:request";

mod request;

#[derive(Debug, Snafu)]
pub enum Error {
    /// 服务返回业务错误（状态码 ≥400 且响应体包含 message 字段）。
    #[snafu(display("{service} request fail, {message}"))]
    Common { service: String, message: String },
    /// 熔断器处于打开状态，未发起请求直接快速失败（保护持续故障的下游）。
    #[snafu(display("{service} circuit breaker open"))]
    CircuitOpen { service: String },
    /// 构建 reqwest 请求失败（如非法 URL、头部格式错误等）。
    #[snafu(display("{service} build http request fail, {source}"))]
    Build {
        service: String,
        source: reqwest::Error,
    },
    /// URL 解析为 `Uri` 失败。
    #[snafu(display("{service} uri fail, {source}"))]
    Uri {
        service: String,
        source: axum::http::uri::InvalidUri,
    },
    /// 发送请求或读取响应体时网络层出错（含超时、连接失败等）。
    #[snafu(display("{service} http request fail, {path} {source}"))]
    Request {
        service: String,
        path: String,
        source: reqwest::Error,
    },
    /// 响应体 JSON 反序列化失败。
    #[snafu(display("{service} json fail, {source}"))]
    Serde {
        service: String,
        source: serde_json::Error,
    },
    /// 目标地址指向内部网络（私网 / 回环 / 链路本地 / 云元数据），被 SSRF 防护拦截。
    #[snafu(display("{service} blocked internal target: {host}"))]
    BlockedTarget { service: String, host: String },
    /// 响应体超过客户端配置的上限，已中止读取。
    #[snafu(display("{service} response too large: exceeds {limit} bytes"))]
    ResponseTooLarge { service: String, limit: usize },
    /// 开启 SSRF 防护的客户端收到 3xx。重定向目标未经 `ensure_public_target` 校验，
    /// 跟随即等于绕过防护，故直接拒绝并把 Location 带出来供排查。
    #[snafu(display("{service} blocked redirect to: {location}"))]
    BlockedRedirect { service: String, location: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let (service, err) = match val {
            Error::Common { service, message } => (service, BaseError::new(message)),
            Error::CircuitOpen { service } => {
                // 熔断快速失败属基础设施保护态，503 + 标记 exception 触发告警
                let err = BaseError::new(format!("{service} circuit breaker open"))
                    .with_status(503)
                    .with_exception(true);
                (service, err)
            }
            Error::Build { service, source } => (service, BaseError::new(source)),
            Error::Uri { service, source } => (service, BaseError::new(source)),
            Error::Request {
                service,
                path,
                source,
            } => {
                let status = source.status().map_or(500, |v| v.as_u16());
                // 超时或连接失败属于基础设施异常，需告警
                let is_network_exception = source.is_timeout() || source.is_connect();
                (
                    service,
                    BaseError::new(source)
                        .with_status(status)
                        .with_exception(is_network_exception)
                        // 此前这里是 `path: _`，把上游辛苦 clone 来的 path 直接丢掉。
                        // reqwest 的错误信息只有 URL 没有我们这侧的路径归一化结果，
                        // 排查时很需要它——放进 extra：5xx 时对客户端脱敏，但完整
                        // Error 仍进 response extensions 供日志读取。
                        .add_extra(format!("path={path}")),
                )
            }
            Error::Serde { service, source } => (service, BaseError::new(source)),
            // 下游回了一个超出预期的巨大响应：502（上游行为异常）并告警——
            // 这既可能是对端故障，也可能是投递目标在有意撑爆我们的内存
            Error::ResponseTooLarge { service, limit } => (
                service,
                BaseError::new(format!("response too large: exceeds {limit} bytes"))
                    .with_status(502)
                    .with_exception(true),
            ),
            Error::BlockedTarget { service, host } => (
                service,
                BaseError::new(format!("blocked internal target: {host}"))
                    .with_status(403)
                    .with_exception(false),
            ),
            // 与 BlockedTarget 同属客户端侧的 SSRF 策略拒绝：403，且不算基础设施异常
            // （下游其实是健康的，只是给了我们一个不能跟随的跳转）
            Error::BlockedRedirect { service, location } => (
                service,
                BaseError::new(format!("blocked redirect to: {location}"))
                    .with_status(403)
                    .with_exception(false),
            ),
        };
        err.with_sub_category(&service).with_category("request")
    }
}

pub use request::*;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// SSRF 策略拒绝（目标内网 / 不可跟随的重定向）统一映射为 403 且不触发告警。
    #[test]
    fn ssrf_rejections_map_to_403_without_alert() {
        for err in [
            Error::BlockedTarget {
                service: "webhook".to_string(),
                host: "169.254.169.254".to_string(),
            },
            Error::BlockedRedirect {
                service: "webhook".to_string(),
                location: "http://169.254.169.254/latest/meta-data/".to_string(),
            },
        ] {
            let base = BaseError::from(err);
            assert_eq!(base.status(), 403);
            assert!(!base.is_exception(), "策略拒绝不应被当成基础设施异常告警");
            assert_eq!(base.category(), "request");
            assert_eq!(base.sub_category(), Some("webhook"));
        }
    }
}
