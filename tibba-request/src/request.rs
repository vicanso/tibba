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

// Import necessary dependencies
use super::Error;
use async_trait::async_trait;
use axum::http::Method;
use axum::http::header::HeaderMap;
use axum::http::uri::Uri;
use bytes::Bytes;
use reqwest::Client as ReqwestClient;
use reqwest::RequestBuilder;
use scopeguard::defer;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tibba_util::{json_get, new_get_duration_ms};
use tracing::info;
type Result<T> = std::result::Result<T, Error>;

// Default empty query and body parameters
static EMPTY_QUERY: Option<&[(&str, &str)]> = None;
static EMPTY_BODY: Option<&[(&str, &str)]> = None;

/// Request parameters structure
/// Generic over query (Q) and body (P) types that must be serializable
#[derive(Clone, Debug, Default)]
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

/// Statistics for HTTP requests
#[derive(Default, Clone, Debug)]
pub struct HttpStats {
    pub method: String,         // HTTP method used
    pub path: String,           // Request path
    pub remote_addr: String,    // Remote address
    pub status: u16,            // Response status code
    pub content_length: usize,  // Response content length
    pub processing: u32,        // Processing time
    pub transfer: u32,          // Transfer time
    pub serde: u32,             // Serialization/deserialization time
    pub total: u32,             // Total request time
    pub tls_version: String,    // TLS version used
    pub tls_not_before: String, // TLS certificate validity start
    pub tls_not_after: String,  // TLS certificate validity end
}

/// HTTP interceptor trait for request/response modification and monitoring
#[async_trait]
pub trait HttpInterceptor: Send + Sync {
    // Handle failed requests (status >= 400)
    async fn fail(&self, _status: u16, _data: &Bytes) -> Result<()> {
        Ok(())
    }
    // Modify outgoing requests
    async fn request(&self, req: RequestBuilder) -> Result<RequestBuilder> {
        Ok(req)
    }
    // Modify incoming responses
    async fn response(&self, data: Bytes) -> Result<Bytes> {
        Ok(data)
    }
    // Handle request completion
    async fn on_done(&self, _stats: &HttpStats, _err: Option<&Error>) -> Result<()> {
        Ok(())
    }
}

/// Common error handling for HTTP responses
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

/// Default interceptor implementation with logging
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
    async fn on_done(&self, stats: &HttpStats, err: Option<&Error>) -> Result<()> {
        let mut error = None;
        if let Some(value) = err {
            error = Some(value.to_string());
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

/// HTTP client configuration
struct ClientConfig {
    service: String,                                     // Service name
    base_url: String,                                    // Base URL for requests
    read_timeout: Option<Duration>,                      // Read timeout
    timeout: Option<Duration>,                           // Overall timeout
    connect_timeout: Option<Duration>,                   // Connection timeout
    pool_idle_timeout: Option<Duration>,                 // Connection pool idle timeout
    pool_max_idle_per_host: usize,                       // Max idle connections per host
    max_processing: Option<u32>,                         // Max concurrent requests
    headers: Option<HeaderMap>,                          // Default headers
    interceptors: Option<Vec<Box<dyn HttpInterceptor>>>, // Request interceptors
}

/// Builder for HTTP client configuration
pub struct ClientBuilder {
    config: ClientConfig,
}

impl ClientBuilder {
    pub fn new(service: &str) -> Self {
        Self {
            config: ClientConfig {
                service: service.to_string(),
                base_url: "".to_string(),
                read_timeout: None,
                timeout: None,
                connect_timeout: None,
                pool_idle_timeout: None,
                pool_max_idle_per_host: 0,
                headers: None,
                interceptors: None,
                max_processing: None,
            },
        }
    }

    pub fn base_url(mut self, base_url: &str) -> Self {
        self.config.base_url = base_url.to_string();
        self
    }

    pub fn interceptor(mut self, interceptor: Box<dyn HttpInterceptor>) -> Self {
        if let Some(interceptors) = &mut self.config.interceptors {
            interceptors.push(interceptor);
        } else {
            self.config.interceptors = Some(vec![interceptor]);
        }
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = Some(timeout);
        self
    }

    pub fn read_timeout(mut self, read_timeout: Duration) -> Self {
        self.config.read_timeout = Some(read_timeout);
        self
    }

    pub fn connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.config.connect_timeout = Some(connect_timeout);
        self
    }

    pub fn pool_idle_timeout(mut self, pool_idle_timeout: Duration) -> Self {
        self.config.pool_idle_timeout = Some(pool_idle_timeout);
        self
    }

    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.config.headers = Some(headers);
        self
    }

    pub fn common_interceptor(self) -> Self {
        let service = self.config.service.clone();
        self.interceptor(Box::new(CommonInterceptor::new(&service)))
    }

    pub fn pool_max_idle_per_host(mut self, pool_max_idle_per_host: usize) -> Self {
        self.config.pool_max_idle_per_host = pool_max_idle_per_host;
        self
    }

    pub fn build(self) -> Result<Client> {
        let mut builder = ReqwestClient::builder()
            .user_agent("tibba-request/1.0")
            .referer(false);
        if let Some(timeout) = self.config.timeout {
            builder = builder.timeout(timeout);
        }
        if let Some(headers) = &self.config.headers {
            builder = builder.default_headers(headers.clone());
        }
        if let Some(read_timeout) = self.config.read_timeout {
            builder = builder.read_timeout(read_timeout);
        }
        if let Some(connect_timeout) = self.config.connect_timeout {
            builder = builder.connect_timeout(connect_timeout);
        }
        if let Some(pool_idle_timeout) = self.config.pool_idle_timeout {
            builder = builder.pool_idle_timeout(pool_idle_timeout);
        }
        if self.config.pool_max_idle_per_host > 0 {
            builder = builder.pool_max_idle_per_host(self.config.pool_max_idle_per_host);
        }

        let client = builder.build().map_err(|e| Error::Build {
            service: self.config.service.clone(),
            source: e,
        })?;
        Ok(Client {
            client,
            config: self.config,
            processing: AtomicU32::new(0),
        })
    }
}

/// HTTP client implementation
pub struct Client {
    client: ReqwestClient, // Underlying reqwest client
    config: ClientConfig,  // Client configuration
    processing: AtomicU32, // Current processing count
}

impl Client {
    /// Constructs full URL from base URL and path
    fn get_url(&self, url: &str) -> String {
        if url.starts_with("http") {
            url.to_string()
        } else {
            self.config.base_url.to_string() + url
        }
    }

    /// Makes raw HTTP request and returns bytes
    async fn raw<Q: Serialize + ?Sized, P: Serialize + ?Sized>(
        &self,
        stats: &mut HttpStats,
        params: Params<'_, Q, P>,
    ) -> Result<Bytes> {
        let processing = self.processing.fetch_add(1, Ordering::Relaxed) + 1;
        defer! {
            self.processing.fetch_sub(1, Ordering::Relaxed);
        };
        if let Some(max_processing) = self.config.max_processing {
            if processing > max_processing {
                return Err(Error::Common {
                    service: self.config.service.clone(),
                    message: "too many requests".to_string(),
                });
            }
        }

        let url = self.get_url(params.url);
        let uri = url.parse::<Uri>().map_err(|e| Error::Uri {
            service: self.config.service.clone(),
            source: e,
        })?;
        let path = uri.path();
        stats.path = path.to_string();
        stats.method = params.method.to_string();

        let mut req = match params.method {
            Method::POST => self.client.post(url),
            Method::PUT => self.client.put(url),
            Method::PATCH => self.client.patch(url),
            Method::DELETE => self.client.delete(url),
            _ => self.client.get(url),
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
        if let Some(interceptors) = &self.config.interceptors {
            for interceptor in interceptors {
                req = interceptor.request(req).await?;
            }
        }
        // TODO dns tcp tls process
        let process_done = new_get_duration_ms();
        let res = req.send().await.map_err(|e| Error::Request {
            service: self.config.service.clone(),
            path: path.to_string(),
            source: e,
        })?;

        stats.processing = process_done();

        if let Some(remote_addr) = res.remote_addr() {
            stats.remote_addr = remote_addr.to_string();
        }

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
        let transfer_done = new_get_duration_ms();
        let mut full = res.bytes().await.map_err(|e| Error::Request {
            service: self.config.service.clone(),
            path: path.to_string(),
            source: e,
        })?;
        stats.transfer = transfer_done();
        stats.content_length = full.len();
        stats.status = status;

        if let Some(interceptors) = &self.config.interceptors {
            if status >= 400 {
                for interceptor in interceptors {
                    interceptor.fail(status, &full).await?;
                }
            }

            for interceptor in interceptors {
                full = interceptor.response(full).await?;
            }
        }
        Ok(full)
    }

    /// Makes HTTP request and deserializes response
    async fn do_request<Q: Serialize + ?Sized, P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        stats: &mut HttpStats,
        params: Params<'_, Q, P>,
    ) -> Result<T> {
        let full = self.raw(stats, params).await?;

        let serde_done = new_get_duration_ms();
        let data = serde_json::from_slice(&full).map_err(|e| Error::Serde {
            service: self.config.service.clone(),
            source: e,
        })?;
        stats.serde = serde_done();
        Ok(data)
    }

    // Public API methods for different HTTP methods
    // GET, POST, etc. with various parameter combinations
    async fn request<Q: Serialize + ?Sized, P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        params: Params<'_, Q, P>,
    ) -> Result<T> {
        let mut stats = HttpStats {
            ..Default::default()
        };
        let done = new_get_duration_ms();
        let result = self.do_request(&mut stats, params).await;
        stats.total = done();
        let mut err = None;
        if let Err(ref e) = result {
            err = Some(e)
        }
        if let Some(interceptors) = &self.config.interceptors {
            for interceptor in interceptors {
                interceptor.on_done(&stats, err).await?;
            }
        }

        result
    }
    pub async fn request_raw<Q: Serialize + ?Sized, P: Serialize + ?Sized>(
        &self,
        params: Params<'_, Q, P>,
    ) -> Result<Bytes> {
        let mut stats = HttpStats {
            ..Default::default()
        };
        let done = new_get_duration_ms();
        let result = self.raw(&mut stats, params).await;
        stats.total = done();
        let mut err = None;
        if let Err(ref e) = result {
            err = Some(e)
        }
        if let Some(interceptors) = &self.config.interceptors {
            for interceptor in interceptors {
                interceptor.on_done(&stats, err).await?;
            }
        }

        result
    }
    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        self.request(Params {
            timeout: None,
            method: Method::GET,
            url,
            query: EMPTY_QUERY,
            body: EMPTY_BODY,
        })
        .await
    }
    pub async fn get_with_query<P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        url: &str,
        query: &P,
    ) -> Result<T> {
        self.request(Params {
            timeout: None,
            method: Method::GET,
            url,
            query: Some(query),
            body: EMPTY_BODY,
        })
        .await
    }
    pub async fn post<P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        url: &str,
        json: &P,
    ) -> Result<T> {
        self.request(Params {
            timeout: None,
            method: Method::POST,
            url,
            query: EMPTY_QUERY,
            body: Some(json),
        })
        .await
    }
    pub async fn post_with_query<
        P: Serialize + ?Sized,
        Q: Serialize + ?Sized,
        T: DeserializeOwned,
    >(
        &self,
        url: &str,
        json: &P,
        query: &Q,
    ) -> Result<T> {
        self.request(Params {
            timeout: None,
            method: Method::POST,
            url,
            query: Some(query),
            body: Some(json),
        })
        .await
    }
}
