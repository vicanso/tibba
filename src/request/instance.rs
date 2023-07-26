use reqwest::{Client, Response};
use serde::de::DeserializeOwned;
use serde::Serialize;
use snafu::{ResultExt, Snafu};
use std::time::Duration;

use crate::error::HttpError;

#[derive(Debug, Snafu)]
pub enum Error {
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
}

type Result<T, E = Error> = std::result::Result<T, E>;
impl From<Error> for HttpError {
    fn from(err: Error) -> Self {
        // 对于部分error单独转换
        match err {
            Error::Build { service, source } => {
                let mut he = HttpError::new_with_category(&source.to_string(), "request");
                he.add_extra(&format!("service:{service}"));
                he.add_extra("category:build");
                he
            }
            Error::Request {
                service,
                path,
                source,
            } => {
                let mut he = HttpError::new_with_category(&source.to_string(), "request");
                he.add_extra(&format!("service:{service}"));
                he.add_extra("category:request");
                he.add_extra(&format!("path:{path}"));
                he
            }
        }
    }
}

type ErrorHandler = Box<dyn Fn(&str, &Response) -> Result<()>>;
pub struct Instance {
    service: String,
    base_url: String,
    timeout: Duration,
    error_handler: ErrorHandler,
}

fn error_handler(service: &str, resp: &Response) -> Result<()> {
    if resp.status().as_u16() >= 400 {}
    Ok(())
}

impl Instance {
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
        // 是否可能传函数
        // 出错
        self.error_handler.as_ref()(&self.service, &resp)?;
        let result = resp.json::<T>().await.context(RequestSnafu {
            service: self.service.clone(),
            path,
        })?;
        Ok(result)
    }
    pub fn new(service: String, base_url: String, timeout: Duration) -> Instance {
        Instance::new_with_error_handler(service, base_url, timeout, Box::new(error_handler))
    }
    pub fn new_with_error_handler(
        service: String,
        base_url: String,
        timeout: Duration,
        error_handler: ErrorHandler,
    ) -> Instance {
        Instance {
            service,
            base_url,
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
