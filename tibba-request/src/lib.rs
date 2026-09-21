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
    /// 上游返回 ≥400，`source` 是从响应体重建出来的上游错误。
    #[snafu(display("{service} upstream returned {status}: {source}"))]
    Upstream {
        service: String,
        /// **上游的**状态码，不是本服务对外的状态码
        status: u16,
        source: BaseError,
    },
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
            // 上游失败 → 本服务回 **502**。
            //
            // 不回 500：那是「我方出 bug」的语义，而这里我方逻辑完好，是依赖挂了；
            // 也不透传上游状态码：上游的 401 不代表**我们的**调用方未登录，照搬
            // 会让客户端收到毫无意义的指令。502 是这件事唯一诚实的表达。
            //
            // 告警分级按上游状态码：5xx 是对方基础设施故障，值得叫人；
            // 4xx 说明我们发出的请求或配置有问题，该修但不是深夜告警。
            Error::Upstream {
                service,
                status,
                source,
            } => {
                let err = BaseError::new(source.message())
                    .with_status(502)
                    .with_exception(status >= 500)
                    // 上游的分类信息进 extra：既留给日志排查，又不会与本服务
                    // 自己的 category / code 混淆（前端按码分流的是我们的码）
                    .add_extra(format!("upstream_status={status}"))
                    .add_extra(format!("upstream_category={}", source.category()));
                let err = match source.code() {
                    Some(code) => err.add_extra(format!("upstream_code={code}")),
                    None => err,
                };
                (service, err)
            }
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

    /// 上游 ≥400 → 本服务 502，且上游的结构化字段要能带回来。
    ///
    /// 回归守卫：此前只取一个 message 字符串、不设状态码，于是上游的 404 也好、
    /// 503 也好，一律变成本服务的 500 并被脱敏。
    #[test]
    fn upstream_failure_maps_to_bad_gateway_with_context() {
        let upstream = BaseError::new("account is locked")
            .with_category("user")
            .with_code("E4031");
        let body = serde_json::to_vec(&upstream).expect("序列化");

        let base = BaseError::from(Error::Upstream {
            service: "billing".to_string(),
            status: 403,
            source: BaseError::from_upstream(403, &body),
        });

        // 我方逻辑完好、是依赖挂了 → 502，而不是 500，也不是照搬上游的 403
        assert_eq!(base.status(), 502);
        assert_eq!(base.category(), "request");
        assert_eq!(base.sub_category(), Some("billing"));
        assert_eq!(base.message(), "account is locked");
        // 上游的分类信息进 extra，供日志排查
        let extra = base.extra().join(",");
        assert!(extra.contains("upstream_status=403"), "{extra}");
        assert!(extra.contains("upstream_category=user"), "{extra}");
        assert!(extra.contains("upstream_code=E4031"), "{extra}");
    }

    /// 告警分级按**上游**状态码：5xx 是对方基础设施故障，4xx 是我们发错了。
    #[test]
    fn only_upstream_server_errors_are_alertable() {
        let make = |status: u16| {
            BaseError::from(Error::Upstream {
                service: "billing".to_string(),
                status,
                source: BaseError::from_upstream(status, b"boom"),
            })
        };
        assert!(make(503).is_exception(), "上游 5xx 应当告警");
        assert!(make(500).is_exception());
        assert!(
            !make(404).is_exception(),
            "上游 4xx 是配置/请求问题，不该深夜告警"
        );
        assert!(!make(429).is_exception());
        // 无论哪种，对外都是 502
        assert_eq!(make(404).status(), 502);
        assert_eq!(make(503).status(), 502);
    }

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
