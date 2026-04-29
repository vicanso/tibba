// Copyright 2025 Tree xie.
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
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let (service, err) = match val {
            Error::Common { service, message } => (service, BaseError::new(message)),
            Error::Build { service, source } => (service, BaseError::new(source)),
            Error::Uri { service, source } => (service, BaseError::new(source)),
            Error::Request {
                service,
                path: _,
                source,
            } => {
                let status = source.status().map_or(500, |v| v.as_u16());
                // 超时或连接失败属于基础设施异常，需告警
                let is_network_exception = source.is_timeout() || source.is_connect();
                (
                    service,
                    BaseError::new(source)
                        .with_status(status)
                        .with_exception(is_network_exception),
                )
            }
            Error::Serde { service, source } => (service, BaseError::new(source)),
        };
        err.with_sub_category(&service).with_category("request")
    }
}

pub use request::*;
