use crate::error::HttpError;
use crate::util::json_get;
use async_trait::async_trait;
use axum::http::uri::Uri;
use axum::http::Method;
use bytes::Bytes;
use chrono::Local;
// use hyper::client::connect::HttpInfo;
use reqwest::{tls::TlsInfo, Client, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::Serialize;
use snafu::{ResultExt, Snafu};
use std::time::Duration;
use tracing::info;
use x509_parser::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Request {service} fail, {message}"))]
    Common { service: String, message: String },
    #[snafu(display("Build {service} http request fail, {source}"))]
    Build {
        service: String,
        source: reqwest::Error,
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
    #[snafu(display("Uri {service} fail, {source}"))]
    Uri {
        service: String,
        source: axum::http::uri::InvalidUri,
    },
}

static ERROR_CATEGORY: &str = "request";

type Result<T, E = Error> = std::result::Result<T, E>;
impl From<Error> for HttpError {
    fn from(err: Error) -> Self {
        // 对于部分error单独转换
        match err {
            Error::Build { service, source } => {
                let mut he = HttpError::new_with_category(&source.to_string(), ERROR_CATEGORY);
                he.add_extra(&format!("service:{service}"));
                he.add_extra("category:build");
                he
            }
            Error::Serde { service, source } => {
                let mut he = HttpError::new_with_category(&source.to_string(), ERROR_CATEGORY);
                he.add_extra(&format!("service:{service}"));
                he.add_extra("category:serde");
                he
            }
            Error::Common { service, message } => {
                let mut he = HttpError::new_with_category(&message, ERROR_CATEGORY);
                he.add_extra(&format!("service:{service}"));
                he.add_extra("category:common");
                he
            }
            Error::Uri { service, source } => {
                let mut he = HttpError::new_with_category(&source.to_string(), ERROR_CATEGORY);
                he.add_extra(&format!("service:{service}"));
                he.add_extra("category:uri");
                he
            }
            Error::Request {
                service,
                path,
                source,
            } => {
                let mut he = HttpError::new_with_category(&source.to_string(), ERROR_CATEGORY);
                he.add_extra(&format!("service:{service}"));
                he.add_extra("category:request");
                he.add_extra(&format!("path:{path}"));
                he
            }
        }
    }
}

#[async_trait]
pub trait HttpInterceptor {
    async fn error(&self, status: u16, data: &Bytes) -> Result<()>;
    async fn request(&self, mut req: RequestBuilder) -> Result<RequestBuilder>;
    async fn response(&self, data: Bytes) -> Result<Bytes>;
    async fn on_done(&self, stats: HttpStats, err: Option<&Error>) -> Result<()>;
}

pub struct Instance<T: HttpInterceptor> {
    c: Client,
    service: String,
    base_url: String,
    interceptor: T,
}

pub async fn handle_error(service: &str, status: u16, data: &Bytes) -> Result<()> {
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
    async fn error(&self, status: u16, data: &Bytes) -> Result<()> {
        handle_error(&self.service, status, data).await
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

fn new_get_duration() -> impl FnOnce() -> u32 {
    let start = Local::now().timestamp_millis();
    move || -> u32 {
        let value = (Local::now().timestamp_millis() - start) as u32;
        // 只要有处理则最小为1，避免与默认值一致
        value.max(1)
    }
}

impl<H: HttpInterceptor> Instance<H> {
    fn get_url(&self, url: &str) -> String {
        self.base_url.to_string() + url
    }
    pub fn new(
        service: &str,
        base_url: &str,
        timeout: Duration,
        interceptor: H,
    ) -> Result<Instance<H>> {
        let c = Client::builder()
            .timeout(timeout)
            .pool_max_idle_per_host(2)
            .tls_info(true)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context(BuildSnafu { service })?;
        Ok(Instance {
            service: service.to_string(),
            base_url: base_url.to_string(),
            c,
            interceptor,
        })
    }
    async fn do_request<Q: Serialize + ?Sized, P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        stats: &mut HttpStats,
        params: Params<'_, Q, P>,
    ) -> Result<T> {
        let url = self.get_url(params.url);
        let uri = url.parse::<Uri>().context(UriSnafu {
            service: &self.service,
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
        req = self.interceptor.request(req).await?;
        // TODO dns tcp tls process
        let process_done = new_get_duration();
        let res = req.send().await.context(RequestSnafu {
            service: &self.service,
            path,
        })?;

        stats.processing = process_done();

        // if let Some(value) = res.extensions().get::<HttpInfo>() {
        //     stats.remote_addr = value.remote_addr().to_string();
        //     stats.local_addr = value.local_addr().to_string();
        // }
        if let Some(value) = res.extensions().get::<TlsInfo>() {
            if let Ok((_, cert)) =
                X509Certificate::from_der(value.peer_certificate().unwrap_or_default())
            {
                stats.tls_version = cert.version.to_string();
                stats.tls_not_before = cert.validity.not_before.to_string();
                stats.tls_not_after = cert.validity.not_after.to_string();
            }
        }

        let status = res.status().as_u16();
        let transfer_done = new_get_duration();
        let mut full = res.bytes().await.context(RequestSnafu {
            service: &self.service,
            path,
        })?;
        stats.transfer = transfer_done();
        stats.content_length = full.len();
        stats.status = status;

        self.interceptor.error(status, &full).await?;
        full = self.interceptor.response(full).await?;
        let serde_done = new_get_duration();
        let data = serde_json::from_slice(&full).context(SerdeSnafu {
            service: &self.service,
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
        self.interceptor.on_done(stats, err).await?;

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
    async fn get_with_query<P: Serialize + ?Sized, T: DeserializeOwned>(
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
