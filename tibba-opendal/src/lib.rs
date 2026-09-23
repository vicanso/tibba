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

/// Postgres 存储 URL 前缀（两种写法都认）。对象存进 `objects` 表。
const POSTGRES_PREFIXES: [&str; 2] = ["postgres://", "postgresql://"];
/// 本地文件系统存储 URL 前缀。
const FS_PREFIX: &str = "file://";

/// OpenDAL 存储配置，`url` 决定后端类型，`schema` 可显式指定协议（如 "http"）。
#[derive(Clone, Validate, Deserialize, Default)]
pub struct OpenDalConfig {
    #[validate(length(min = 10))]
    pub url: String,
    #[serde(default)]
    pub schema: String,
}

/// 手写 `Debug`：`url` 里带着凭据——S3 的 `secret_access_key` 在查询串里，
/// 数据库后端的口令在 userinfo 里。derive 会把它们原样打进日志。
impl std::fmt::Debug for OpenDalConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenDalConfig")
            .field("url", &redact_url(&self.url))
            .field("schema", &self.schema)
            .finish()
    }
}

/// 去掉 URL 里所有可能含凭据的部分：userinfo 与整个查询串。
///
/// 查询串整体丢弃而不是逐个参数打码：S3 的凭据参数名有好几种写法，
/// 白名单保留哪些参数迟早会漏；排查连接问题时有 scheme + host + path 已经足够。
fn redact_url(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some(parts) => parts,
        None => return url.split('?').next().unwrap_or_default().to_string(),
    };
    let rest = rest.split('?').next().unwrap_or_default();
    let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    let suffix = if url.contains('?') { "?<redacted>" } else { "" };
    format!("{scheme}://{rest}{suffix}")
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
            Error::OpenDal { source } => opendal_error(*source),
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

/// 按 OpenDAL 的错误类别映射状态码。
///
/// 此前一律「500 + 告警」，于是下载一个不存在的对象、或在 fs 后端上调 presign，
/// 都被当成服务端故障报警。其中只有「后端不可用 / 内部错误」才是真的故障。
fn opendal_error(source: opendal::Error) -> BaseError {
    use opendal::ErrorKind;
    match source.kind() {
        ErrorKind::NotFound => BaseError::new("object not found")
            .with_sub_category("not_found")
            .with_status(404),
        // 后端不支持该能力（如 fs 上 presign）：请求方选错了操作，不是故障
        ErrorKind::Unsupported => BaseError::new(source.to_string())
            .with_sub_category("unsupported")
            .with_status(501),
        ErrorKind::RateLimited => BaseError::new("storage backend rate limited")
            .with_sub_category("rate_limited")
            .with_status(503),
        _ => BaseError::new(source)
            .with_sub_category("backend")
            .with_status(500)
            .with_exception(true),
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

/// 将 OpenDAL builder 组装成统一的 `Storage`：
/// 1) 套 `MimeGuessLayer` 让对象自动带 Content-Type
/// 2) 错误统一走 `OpenDalSnafu`
///
/// 各后端工厂只负责拼装自己的 builder，公共的 Operator 构造与 layer 装载
/// 集中在这里，避免 4 处复制相同的三行模板。
fn finalize_dal<B>(builder: B) -> Result<Storage>
where
    B: opendal::Builder,
{
    // opendal 0.58 起 `Operator::new` 直接返回构造完成的 Operator，不再需要 `finish()`
    let dal = Operator::new(builder)
        .context(OpenDalSnafu)?
        .layer(MimeGuessLayer::default());
    Ok(Storage::new(dal))
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
    finalize_dal(builder)
}

/// 从 Postgres 连接字符串创建数据库存储后端，对象存进 `objects` 表
/// （`"key"` / `value BYTEA`，见 baseline 迁移）。
///
/// 此前是 MySQL 后端：本项目早已全面迁到 Postgres，`objects` 表的 DDL 也是
/// PG 语法，MySQL 后端根本用不上它——要用它就得额外维护一套 MySQL。
fn new_postgres_dal(url: &str) -> Result<Storage> {
    finalize_dal(
        opendal::services::Postgresql::default()
            .connection_string(url)
            .table("objects")
            .key_field("key")
            .value_field("value"),
    )
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
    finalize_dal(opendal::services::Fs::default().root(&resolve_path(root)))
}

/// 从 HTTP URL 创建只读 HTTP 存储后端。
fn new_http_dal(url: &str) -> Result<Storage> {
    finalize_dal(opendal::services::Http::default().endpoint(url))
}

/// 根据配置 URL 自动选择存储后端并创建 Storage 实例。
/// - `postgres://` / `postgresql://` → Postgres 后端（`objects` 表）
/// - `file://`  → 本地文件系统后端
/// - `schema = "http"` → HTTP 只读后端
/// - 其余 → S3 兼容后端
pub fn new_opendal_storage(config: &Config) -> Result<Storage> {
    let opendal_config = new_opendal_config(config)?;
    let url = opendal_config.url.as_str();
    new_opendal_storage_from_url(url, Some(&opendal_config.schema))
}

/// 根据 URL 和 schema 自动选择存储后端并创建 Storage 实例。
pub fn new_opendal_storage_from_url(url: &str, schema: Option<&str>) -> Result<Storage> {
    match url {
        url if POSTGRES_PREFIXES.iter().any(|p| url.starts_with(p)) => new_postgres_dal(url),
        url if url.starts_with(FS_PREFIX) => new_fs_dal(url),
        url if schema == Some("http") => new_http_dal(url),
        _ => new_s3_dal(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Debug 不得泄漏 S3 密钥或数据库口令。
    #[test]
    fn debug_redacts_credentials() {
        let s3 = OpenDalConfig {
            url: "https://s3.example.com/root?bucket=b&access_key_id=AKIA&secret_access_key=SeCrEt"
                .to_string(),
            schema: String::new(),
        };
        let debug = format!("{s3:?}");
        assert!(
            !debug.contains("SeCrEt") && !debug.contains("AKIA"),
            "{debug}"
        );
        assert!(debug.contains("s3.example.com/root?<redacted>"), "{debug}");

        let pg = OpenDalConfig {
            url: "postgres://user:p@ss@db:5432/app".to_string(),
            schema: String::new(),
        };
        let debug = format!("{pg:?}");
        assert!(!debug.contains("p@ss"), "{debug}");
        assert!(debug.contains("postgres://db:5432/app"), "{debug}");
    }

    #[test]
    fn redact_url_keeps_structure() {
        assert_eq!(redact_url("file:///var/data"), "file:///var/data");
        assert_eq!(redact_url("https://h/p?x=1"), "https://h/p?<redacted>");
        assert_eq!(redact_url("no-scheme?secret=1"), "no-scheme");
    }

    /// 对象不存在是 404，不是「500 + 告警」。
    #[test]
    fn opendal_errors_map_by_kind() {
        use opendal::ErrorKind;
        let nf = opendal_error(opendal::Error::new(ErrorKind::NotFound, "missing"));
        assert_eq!(nf.status(), 404);
        assert!(!nf.is_exception());

        let unsupported = opendal_error(opendal::Error::new(ErrorKind::Unsupported, "presign"));
        assert_eq!(unsupported.status(), 501);
        assert!(!unsupported.is_exception());

        let other = opendal_error(opendal::Error::new(ErrorKind::Unexpected, "boom"));
        assert_eq!(other.status(), 500);
        assert!(other.is_exception());
    }
}
