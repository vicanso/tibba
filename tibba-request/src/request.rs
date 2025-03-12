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

use async_trait::async_trait;
use axum::http::Method;
use axum::http::uri::Uri;
use bytes::Bytes;
use reqwest::{Client, RequestBuilder, tls::TlsInfo};
use serde::Serialize;
use serde::de::DeserializeOwned;
use snafu::{ResultExt, Snafu};
use std::time::Duration;
use std::time::Instant;
use tibba_util::json_get;
use tracing::info;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("request {service} fail, {message}"))]
    Common { service: String, message: String },
    #[snafu(display("build {service} http request fail, {source}"))]
    Build {
        service: String,
        source: reqwest::Error,
    },
    #[snafu(display("uri {service} fail, {source}"))]
    Uri {
        service: String,
        source: axum::http::uri::InvalidUri,
    },
    #[snafu(display("Http {service} request fail, {path} {source}"))]
    Request {
        service: String,
        path: String,
        source: reqwest::Error,
    },
    #[snafu(display("Json {service} fail, {source}"))]
    Serde {
        service: String,
        source: serde_json::Error,
    },
}
type Result<T> = std::result::Result<T, Error>;

static EMPTY_QUERY: Option<&[(&str, &str)]> = None;
static EMPTY_BODY: Option<&[(&str, &str)]> = None;

#[derive(Default, Clone, Debug)]
pub struct Params<'a, Q, P>
where
    Q: Serialize + ?Sized,
    P: Serialize + ?Sized,
{
    pub method: Method,
    pub timeout: Option<Duration>,
    pub query: Option<&'a Q>,
    pub body: Option<&'a P>,
    pub url: &'a str,
}

#[derive(Default, Clone, Debug)]
pub struct HttpStats {
    pub method: String,
    pub path: String,
    pub remote_addr: String,
    pub local_addr: String,
    pub status: u16,
    pub content_length: usize,
    pub processing: u32,
    pub transfer: u32,
    pub serde: u32,
    pub total: u32,
    pub tls_version: String,
    pub tls_not_before: String,
    pub tls_not_after: String,
}

#[async_trait]
pub trait HttpInterceptor {
    async fn fail(&self, _status: u16, _data: &Bytes) -> Result<()> {
        Ok(())
    }
    async fn request(&self, req: RequestBuilder) -> Result<RequestBuilder> {
        Ok(req)
    }
    async fn response(&self, data: Bytes) -> Result<Bytes> {
        Ok(data)
    }
    async fn on_done(&self, _stats: HttpStats, _err: Option<&Error>) -> Result<()> {
        Ok(())
    }
}

pub async fn handle_fail(service: &str, status: u16, data: &Bytes) -> Result<()> {
    if status >= 400 {
        let mut message = json_get(data, "message");
        if message.is_empty() {
            message = "unknown error".to_string();
        }
        return Err(Error::Common {
            service: service.to_string(),
            message,
        });
    }
    Ok(())
}

pub struct CommonInterceptor {
    service: String,
}

impl CommonInterceptor {
    pub fn new(service: &str) -> CommonInterceptor {
        CommonInterceptor {
            service: service.to_string(),
        }
    }
}

#[async_trait]
impl HttpInterceptor for CommonInterceptor {
    async fn fail(&self, status: u16, data: &Bytes) -> Result<()> {
        handle_fail(&self.service, status, data).await
    }
    async fn request(&self, req: RequestBuilder) -> Result<RequestBuilder> {
        Ok(req)
    }
    async fn response(&self, data: Bytes) -> Result<Bytes> {
        Ok(data)
    }
    async fn on_done(&self, stats: HttpStats, err: Option<&Error>) -> Result<()> {
        let mut error = "".to_string();
        if let Some(value) = err {
            error = value.to_string();
        }
        info!(
            service = self.service,
            method = stats.method,
            path = stats.path,
            status = stats.status,
            remote_addr = stats.remote_addr,
            content_length = stats.content_length,
            processing = stats.processing,
            transfer = stats.transfer,
            serde = stats.serde,
            total = stats.total,
            error,
        );
        Ok(())
    }
}

#[derive(Default, Clone, Debug)]
pub struct InstanceClientBuilder<T: HttpInterceptor> {
    service: String,
    base_url: String,
    timeout: Duration,
    interceptor: Option<T>,
}

impl<T: HttpInterceptor> InstanceClientBuilder<T> {
    pub fn builder(base_url: &str) -> InstanceClientBuilder<T> {
        InstanceClientBuilder {
            service: "".to_string(),
            base_url: base_url.to_string(),
            timeout: Duration::from_secs(10),
            interceptor: None,
        }
    }
    pub fn service(mut self, service: &str) -> InstanceClientBuilder<T> {
        self.service = service.to_string();
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> InstanceClientBuilder<T> {
        self.timeout = timeout;
        self
    }
    pub fn interceptor(mut self, interceptor: T) -> InstanceClientBuilder<T> {
        self.interceptor = Some(interceptor);
        self
    }
    pub fn build(self) -> Result<Instance<T>> {
        let c = Client::builder()
            .timeout(self.timeout)
            .pool_max_idle_per_host(2)
            .tls_info(true)
            // TODO connect timeout from config
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context(BuildSnafu {
                service: self.service.clone(),
            })?;
        Ok(Instance { c, config: self })
    }
}

pub struct Instance<T: HttpInterceptor> {
    c: Client,
    config: InstanceClientBuilder<T>,
}

fn new_get_duration() -> impl FnOnce() -> u32 {
    let start = Instant::now();
    move || -> u32 {
        let value = start.elapsed().as_millis() as u32;
        // 只要有处理则最小为1，避免与默认值一致
        value.max(1)
    }
}

impl<H: HttpInterceptor + Send + Sync> Instance<H> {
    pub fn builder(base_url: &str) -> InstanceClientBuilder<H> {
        InstanceClientBuilder::builder(base_url)
    }
    fn get_url(&self, url: &str) -> String {
        self.config.base_url.to_string() + url
    }
    async fn do_request<Q: Serialize + ?Sized, P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        stats: &mut HttpStats,
        params: Params<'_, Q, P>,
    ) -> Result<T> {
        let url = self.get_url(params.url);
        let uri = url.parse::<Uri>().context(UriSnafu {
            service: &self.config.service,
        })?;
        let path = uri.path();
        stats.path = path.to_string();
        stats.method = params.method.to_string();

        let mut req = match params.method {
            Method::POST => self.c.post(url),
            Method::PUT => self.c.put(url),
            Method::PATCH => self.c.patch(url),
            Method::DELETE => self.c.delete(url),
            _ => self.c.get(url),
        };
        if let Some(value) = params.timeout {
            req = req.timeout(value);
        }

        if let Some(value) = params.query {
            req = req.query(value);
        }
        if let Some(value) = params.body {
            req = req.json(value);
        }
        if let Some(interceptor) = &self.config.interceptor {
            req = interceptor.request(req).await?;
        }
        // TODO dns tcp tls process
        let process_done = new_get_duration();
        let res = req.send().await.context(RequestSnafu {
            service: &self.config.service,
            path,
        })?;

        stats.processing = process_done();

        // if let Some(value) = res.extensions().get::<HttpInfo>() {
        //     stats.remote_addr = value.remote_addr().to_string();
        //     stats.local_addr = value.local_addr().to_string();
        // }
        // if let Some(value) = res.extensions().get::<TlsInfo>() {
        //     if let Ok((_, cert)) =
        //         X509Certificate::from_der(value.peer_certificate().unwrap_or_default())
        //     {
        //         stats.tls_version = cert.version.to_string();
        //         stats.tls_not_before = cert.validity.not_before.to_string();
        //         stats.tls_not_after = cert.validity.not_after.to_string();
        //     }
        // }

        let status = res.status().as_u16();
        let transfer_done = new_get_duration();
        let mut full = res.bytes().await.context(RequestSnafu {
            service: &self.config.service,
            path,
        })?;
        stats.transfer = transfer_done();
        stats.content_length = full.len();
        stats.status = status;

        if let Some(interceptor) = &self.config.interceptor {
            interceptor.fail(status, &full).await?;
            full = interceptor.response(full).await?;
        }
        let serde_done = new_get_duration();
        let data = serde_json::from_slice(&full).context(SerdeSnafu {
            service: &self.config.service,
        })?;
        stats.serde = serde_done();
        Ok(data)
    }
    async fn request<Q: Serialize + ?Sized, P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        params: Params<'_, Q, P>,
    ) -> Result<T> {
        let mut stats = HttpStats {
            ..Default::default()
        };
        let done = new_get_duration();
        let result = self.do_request(&mut stats, params).await;
        stats.total = done();
        let mut err = None;
        if let Err(ref e) = result {
            err = Some(e)
        }
        if let Some(interceptor) = &self.config.interceptor {
            interceptor.on_done(stats, err).await?;
        }

        result
    }
    // add builder function
    // pub fn new(
    //     service: &str,
    //     base_url: &str,
    //     timeout: Duration,
    //     interceptor: H,
    // ) -> Result<Instance<H>> {
    //     let c = Client::builder()
    //         .timeout(timeout)
    //         .pool_max_idle_per_host(2)
    //         .tls_info(true)
    //         // TODO connect timeout from config
    //         .connect_timeout(Duration::from_secs(10))
    //         .build()
    //         .context(BuildSnafu { service })?;
    //     Ok(Instance {
    //         service: service.to_string(),
    //         base_url: base_url.to_string(),
    //         c,
    //         interceptor,
    //     })
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    #[test]
    fn test_instance_client_builder() {
        let builder: InstanceClientBuilder<CommonInterceptor> =
            InstanceClientBuilder::builder("https://api.example.com")
                .service("test")
                .timeout(Duration::from_secs(1));
        assert_eq!(builder.base_url, "https://api.example.com");
        assert_eq!(builder.service, "test");
        assert_eq!(builder.timeout, Duration::from_secs(1));
        assert_eq!(builder.interceptor.is_none(), true);

        let instance = builder.build().unwrap();
        assert_eq!(instance.config.service, "test");
    }
}
