use async_trait::async_trait;
use bytes::Bytes;
use reqwest::{Client, Response};
use serde::de::DeserializeOwned;
use serde::Serialize;
use snafu::{ResultExt, Snafu};
use std::time::Duration;

use crate::error::HttpError;
use crate::util::json_get;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Request fail, service:{service}, {message}"))]
    Common { service: String, message: String },
    #[snafu(display("Build http request fail, service:{service}, {source}"))]
    Build {
        service: String,
        source: reqwest::Error,
    },
    #[snafu(display("Http request fail, service:{service}, path:{path} {source}"))]
    Request {
        service: String,
        path: String,
        source: reqwest::Error,
    },
    #[snafu(display("Json fail, {source}"))]
    Serde { source: serde_json::Error },
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
            Error::Serde { source } => {
                let mut he = HttpError::new_with_category(&source.to_string(), ERROR_CATEGORY);
                // he.add_extra(&format!("service:{service}"));
                he.add_extra("category:serde");
                he
            }
            Error::Common { service, message } => {
                let mut he = HttpError::new_with_category(&message, ERROR_CATEGORY);
                he.add_extra(&format!("service:{service}"));
                he.add_extra("category:common");
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
pub trait HttpErrorHandler {
    async fn handle(&self, service: &str, status: u16, data: &Bytes) -> Result<()>;
}

pub struct Instance<T: HttpErrorHandler> {
    service: String,
    base_url: String,
    timeout: Duration,
    error_handler: T,
}

pub async fn error_handler(service: &str, status: u16, data: &Bytes) -> Result<()> {
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

pub struct CommonErrorHandler {}

#[async_trait]
impl HttpErrorHandler for CommonErrorHandler {
    async fn handle(&self, service: &str, status: u16, data: &Bytes) -> Result<()> {
        error_handler(service, status, data).await
    }
}

impl<E: HttpErrorHandler> Instance<E> {
    fn get_url(&self, url: &str) -> String {
        self.base_url.to_string() + url
    }
    fn get_conn(&self) -> Result<Client> {
        let c = Client::builder()
            .timeout(self.timeout)
            .build()
            .context(BuildSnafu {
                service: self.service.clone(),
            })?;
        Ok(c)
    }
    async fn handle_response<T: DeserializeOwned>(&self, resp: Response) -> Result<T> {
        let path = resp.url().path().to_string();
        let status = resp.status().as_u16();
        let full = resp.bytes().await.context(RequestSnafu {
            service: self.service.clone(),
            path,
        })?;

        self.error_handler
            .handle(&self.service, status, &full)
            .await?;
        serde_json::from_slice(&full).context(SerdeSnafu {})
    }

    pub fn new(service: &str, base_url: &str, timeout: Duration, error_handler: E) -> Instance<E> {
        Instance {
            service: service.to_string(),
            base_url: base_url.to_string(),
            timeout,
            error_handler,
        }
    }
    pub async fn get<P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        url: &str,
        query: &P,
    ) -> Result<T> {
        let c = self.get_conn()?;
        let resp = c
            .get(self.get_url(url))
            .query(query)
            .send()
            .await
            .context(RequestSnafu {
                service: self.service.clone(),
                path: url.to_string(),
            })?;
        self.handle_response(resp).await
    }
    pub async fn post<P: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        url: &str,
        json: &P,
        query: &P,
    ) -> Result<T> {
        let c = self.get_conn()?;
        let resp = c
            .post(self.get_url(url))
            .json(json)
            .query(query)
            .send()
            .await
            .context(RequestSnafu {
                service: self.service.clone(),
                path: url.to_string(),
            })?;
        self.handle_response(resp).await
    }
}
