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
use snafu::{ResultExt, Snafu};
use std::path::PathBuf;
use tibba_config::Config;
use tibba_error::Error as BaseError;
use tibba_util::parse_uri;
use validator::Validate;

mod storage;

pub use storage::*;

/// MySQL 存储 URL 前缀。
const MYSQL_PREFIX: &str = "mysql://";
/// 本地文件系统存储 URL 前缀。
const FS_PREFIX: &str = "file://";

/// OpenDAL 存储配置，`url` 决定后端类型，`schema` 可显式指定协议（如 "http"）。
#[derive(Debug, Clone, Validate, Deserialize, Default)]
pub struct OpenDalConfig {
    #[validate(length(min = 10))]
    pub url: String,
    #[serde(default)]
    pub schema: String,
}

/// 从应用配置中读取并校验 OpenDalConfig。
fn new_opendal_config(config: &Config) -> Result<OpenDalConfig> {
    let open_dal_config = config
        .try_deserialize::<OpenDalConfig>()
        .context(ConfigSnafu)?;
    open_dal_config.validate().context(ValidateSnafu)?;
    Ok(open_dal_config)
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("open dal {source}"))]
    OpenDal {
        #[snafu(source(from(opendal::Error, Box::new)))]
        source: Box<opendal::Error>,
    },
    #[snafu(display("config error: {source}"))]
    Config {
        #[snafu(source(from(tibba_config::Error, Box::new)))]
        source: Box<tibba_config::Error>,
    },
    #[snafu(display("parse uri error: {source}"))]
    ParseUri {
        #[snafu(source(from(tibba_util::Error, Box::new)))]
        source: Box<tibba_util::Error>,
    },
    #[snafu(display("validate {source}"))]
    Validate {
        #[snafu(source(from(validator::ValidationErrors, Box::new)))]
        source: Box<validator::ValidationErrors>,
    },
    /// 其他无效参数或配置错误。
    #[snafu(display("{message}"))]
    Invalid { message: String },
}

type Result<T, E = Error> = std::result::Result<T, E>;

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::OpenDal { source } => BaseError::new(source).with_exception(true),
            Error::Config { source } => BaseError::new(*source).with_sub_category("config"),
            Error::ParseUri { source } => BaseError::new(*source)
                .with_sub_category("parse_uri")
                .with_exception(true),
            Error::Validate { source } => BaseError::new(*source)
                .with_sub_category("validate")
                .with_exception(true),
            Error::Invalid { message } => BaseError::new(message).with_exception(true),
        };
        err.with_category("open_dal")
    }
}

/// S3 连接参数，从 URL 查询字符串中解析。
#[derive(Deserialize, Debug, PartialEq)]
struct S3Params {
    bucket: String,
    region: Option<String>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
}

/// 从 S3 兼容 URL 创建 S3 存储后端。
fn new_s3_dal(url: &str) -> Result<Storage> {
    let parsed = parse_uri::<S3Params>(url).context(ParseUriSnafu)?;
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
        .context(OpenDalSnafu)?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage::new(dal))
}

/// 从 MySQL 连接字符串创建 MySQL 存储后端，使用 `objects` 表存储对象数据。
fn new_mysql_dal(url: &str) -> Result<Storage> {
    let builder = opendal::services::Mysql::default()
        .connection_string(url)
        .table("objects");
    let dal = Operator::new(builder)
        .context(OpenDalSnafu)?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage::new(dal))
}

/// 将路径字符串规范化为绝对路径，支持 `~/` 家目录前缀展开。
#[inline]
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

/// 从 `file://` URL 创建本地文件系统存储后端，根路径需至少 2 个字符。
fn new_fs_dal(url: &str) -> Result<Storage> {
    let root = url.strip_prefix(FS_PREFIX).unwrap_or_default();
    if root.len() < 2 {
        return Err(Error::Invalid {
            message: "root is empty".to_string(),
        });
    }
    let builder = opendal::services::Fs::default().root(&resolve_path(root));
    let dal = Operator::new(builder)
        .context(OpenDalSnafu)?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage::new(dal))
}

/// 从 HTTP URL 创建只读 HTTP 存储后端。
fn new_http_dal(url: &str) -> Result<Storage> {
    let builder = opendal::services::Http::default().endpoint(url);
    let dal = Operator::new(builder)
        .context(OpenDalSnafu)?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage::new(dal))
}

/// 根据配置 URL 自动选择存储后端并创建 Storage 实例。
/// - `mysql://` → MySQL 后端
/// - `file://`  → 本地文件系统后端
/// - `schema = "http"` → HTTP 只读后端
/// - 其余 → S3 兼容后端
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
