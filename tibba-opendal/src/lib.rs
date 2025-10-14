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
use path_absolutize::Absolutize;
use serde::Deserialize;
use snafu::Snafu;
use std::path::PathBuf;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_util::parse_uri;
use validator::Validate;

mod storage;

pub use storage::*;

const MYSQL_PREFIX: &str = "mysql://";
const FS_PREFIX: &str = "file://";

#[derive(Debug, Clone, Default, Validate)]
pub struct OpenDalConfig {
    #[validate(length(min = 10))]
    pub url: String,
    pub schema: String,
}

fn new_opendal_config(config: &Config) -> Result<OpenDalConfig> {
    let url = config.get_str("url", "");
    let schema = config.get_str("schema", "");
    let open_dal_config = OpenDalConfig { url, schema };
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
    #[snafu(display("{message}"))]
    Invalid { message: String },
}

impl From<Error> for BaseError {
    fn from(source: Error) -> Self {
        let err = match source {
            Error::OpenDal { source } => BaseError::new(source).with_exception(true),
            Error::ParseUrl { source } => BaseError::new(source)
                .with_sub_category("parse_url")
                .with_exception(true),
            Error::Validate { source } => BaseError::new(source)
                .with_sub_category("validate")
                .with_exception(true),
            Error::Invalid { message } => BaseError::new(message).with_exception(true),
        };
        err.with_category("open_dal")
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Deserialize, Debug, PartialEq)]
struct S3Params {
    bucket: String,
    region: Option<String>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
}

/// Create a new S3 storage.
fn new_s3_dal(url: &str) -> Result<Storage> {
    let parsed = parse_uri::<S3Params>(url).map_err(|e| Error::Invalid {
        message: e.to_string(),
    })?;
    // let params = parse_params(url)?;
    let mut builder = opendal::services::S3::default().endpoint(&parsed.endpoint());
    if let Some(path) = parsed.path {
        builder = builder.root(path);
    }
    let query = parsed.query;
    builder = builder.bucket(&query.bucket);
    if let Some(region) = &query.region {
        builder = builder.region(region);
    }
    if let Some(access_key_id) = &query.access_key_id {
        builder = builder.access_key_id(access_key_id);
    }
    if let Some(secret_access_key) = &query.secret_access_key {
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

/// Create a new MySQL storage.
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

fn resolve_path(path_str: &str) -> String {
    if path_str.is_empty() {
        return String::new();
    }
    let path = if let Some(stripped) = path_str.strip_prefix("~/") {
        dirs::home_dir()
            .map(|home| home.join(stripped))
            .unwrap_or_else(|| PathBuf::from(path_str))
    } else {
        PathBuf::from(path_str)
    };

    path.absolutize().map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |p| p.to_string_lossy().into_owned(),
    )
}

fn new_fs_dal(url: &str) -> Result<Storage> {
    let root = url.strip_prefix(FS_PREFIX).unwrap_or_default();
    if root.len() < 2 {
        return Err(Error::Invalid {
            message: "root is empty".to_string(),
        });
    }

    let builder = opendal::services::Fs::default().root(&resolve_path(&root));
    let dal = Operator::new(builder)
        .map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage::new(dal))
}

fn new_http_dal(url: &str) -> Result<Storage> {
    let builder = opendal::services::Http::default().endpoint(url);
    let dal = Operator::new(builder)
        .map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage::new(dal))
}

/// Create a new storage from config.
/// If it's a MySQL URL, it will create a MySQL storage.
/// Otherwise, it will create a S3 storage.
pub fn new_opendal_storage(config: &Config) -> Result<Storage> {
    let opendal_config = new_opendal_config(config)?;
    let url = opendal_config.url.as_str();
    match url {
        url if url.starts_with(MYSQL_PREFIX) => new_mysql_dal(url),
        url if url.starts_with(FS_PREFIX) => new_fs_dal(url),
        url if &opendal_config.schema == "http" => new_http_dal(url),
        _ => new_s3_dal(url),
    }
}
