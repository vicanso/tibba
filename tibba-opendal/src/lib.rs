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

use opendal::Operator;
use opendal::layers::MimeGuessLayer;
use snafu::Snafu;
use std::collections::HashMap;
use tibba_config::Config;
use tibba_error::{Error as BaseError, new_error};
use url::Url;
use validator::Validate;

mod storage;

pub use storage::*;

#[derive(Debug, Clone, Default, Validate)]
pub struct OpenDalConfig {
    #[validate(length(min = 10))]
    pub url: String,
}

fn new_opendal_config(config: &Config) -> Result<OpenDalConfig> {
    let url = config.get_from_env_first("url", None);
    let open_dal_config = OpenDalConfig { url };
    open_dal_config
        .validate()
        .map_err(|e| Error::Validate { source: e })?;
    Ok(open_dal_config)
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("open dal {source}"))]
    OpenDal { source: Box<opendal::Error> },
    #[snafu(display("parse url {source}"))]
    ParseUrl { source: url::ParseError },
    #[snafu(display("validate {source}"))]
    Validate { source: validator::ValidationErrors },
}

impl From<Error> for BaseError {
    fn from(source: Error) -> Self {
        let error_category = "open_dal";
        match source {
            Error::OpenDal { source } => {
                let he = new_error(source)
                    .with_category(error_category)
                    .with_sub_category("opendal")
                    .with_exception(true);
                he.into()
            }
            Error::ParseUrl { source } => {
                let he = new_error(source)
                    .with_category(error_category)
                    .with_sub_category("parse_url")
                    .with_exception(true);
                he.into()
            }
            Error::Validate { source } => {
                let he = new_error(source)
                    .with_category(error_category)
                    .with_sub_category("validate")
                    .with_exception(true);
                he.into()
            }
        }
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

struct StorageParams {
    endpoint: String,
    path: String,
    query: HashMap<String, String>,
}

fn parse_params(url: &str) -> Result<StorageParams> {
    let info = Url::parse(url).map_err(|e| Error::ParseUrl { source: e })?;
    let mut endpoint = format!(
        "{}://{}",
        info.scheme(),
        info.host().map(|v| v.to_string()).unwrap_or_default()
    );
    if let Some(port) = info.port() {
        endpoint = format!("{}:{}", endpoint, port);
    }

    let mut query = HashMap::new();
    info.query_pairs().for_each(|(k, v)| {
        query.insert(k.to_string(), v.to_string());
    });

    Ok(StorageParams {
        endpoint,
        path: info.path().to_string(),
        query,
    })
}

fn new_s3_dal(url: &str) -> Result<Storage> {
    let params = parse_params(url)?;
    let mut builder = opendal::services::S3::default().endpoint(&params.endpoint);
    if !params.path.is_empty() {
        builder = builder.root(&params.path);
    }
    if let Some(bucket) = params.query.get("bucket") {
        builder = builder.bucket(bucket);
    }
    if let Some(region) = params.query.get("region") {
        builder = builder.region(region);
    }
    if let Some(access_key_id) = params.query.get("access_key_id") {
        builder = builder.access_key_id(access_key_id);
    }
    if let Some(secret_access_key) = params.query.get("secret_access_key") {
        builder = builder.secret_access_key(secret_access_key);
    }

    let dal = opendal::Operator::new(builder)
        .map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage::new(dal))
}

fn new_mysql_dal(url: &str) -> Result<Storage> {
    let builder = opendal::services::Mysql::default()
        .connection_string(url)
        .table("objects");

    let dal = Operator::new(builder)
        .map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage::new(dal))
}

pub fn new_opendal_storage(config: &Config) -> Result<Storage> {
    let opendal_config = new_opendal_config(config)?;
    let url = opendal_config.url.as_str();
    if url.starts_with("mysql://") {
        new_mysql_dal(url)
    } else {
        new_s3_dal(url)
    }
}
