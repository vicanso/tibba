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

#[cfg(any(feature = "fs", feature = "s3", feature = "http"))]
use opendal::Operator;
#[cfg(any(feature = "fs", feature = "s3", feature = "http"))]
use opendal::layers::MimeGuessLayer;
use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use std::fmt;
#[cfg(feature = "fs")]
use std::path::PathBuf;
use tibba_config::Config;
use tibba_error::Error as BaseError;
#[cfg(feature = "s3")]
use tibba_util::parse_uri;
use validator::Validate;

mod storage;

pub use storage::*;

/// 本地文件系统存储 URL 前缀。
#[cfg(feature = "fs")]
const FS_PREFIX: &str = "file://";

/// OpenDAL 存储配置，`url` 决定后端类型，`schema` 可显式指定协议（如 "http"）。
#[derive(Clone, Validate, Deserialize, Default)]
#[validate(schema(function = "validate_opendal_config"))]
pub struct OpenDalConfig {
    pub url: String,
    #[serde(default)]
    pub schema: String,
}

fn validate_opendal_config(
    cfg: &OpenDalConfig,
) -> std::result::Result<(), validator::ValidationError> {
    if cfg.url.trim().len() < 6 {
        return Err(validator::ValidationError::new("opendal url is too short"));
    }
    Ok(())
}

/// 手写 `Debug`：`url` 里带着凭据——S3 的 `secret_access_key` 在查询串里，
/// 数据库后端的口令在 userinfo 里。derive 会把它们原样打进日志。
impl fmt::Debug for OpenDalConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenDalConfig")
            .field("url", &redact_url(&self.url))
            .field("schema", &self.schema)
            .finish()
    }
}

/// 去掉 URL 里所有可能含凭据的部分：userinfo、整个查询串与 `#fragment`。
///
/// 只在第一个 `/` 之前的 authority 段内查找 `@`，避免路径里合法的 `@` 被误切掉。
pub(crate) fn redact_url(url: &str) -> String {
    let no_frag = url.split('#').next().unwrap_or_default();
    let has_query = no_frag.contains('?');
    let base = no_frag.split('?').next().unwrap_or_default();
    let suffix = if has_query { "?<redacted>" } else { "" };

    let Some((scheme, rest)) = base.split_once("://") else {
        return base.to_string();
    };
    let (authority, path_suffix) = match rest.split_once('/') {
        Some((auth, p)) => (auth, Some(p)),
        None => (rest, None),
    };
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    match path_suffix {
        Some(p) => format!("{scheme}://{host}/{p}{suffix}"),
        None => format!("{scheme}://{host}{suffix}"),
    }
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
    #[cfg(feature = "s3")]
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
    /// 非法对象存储路径（如目录穿越、控制字符等，映射 400）。
    #[snafu(display("{message}"))]
    InvalidPath { message: String },
    /// 其他无效参数或配置错误。
    #[snafu(display("{message}"))]
    Invalid { message: String },
}

type Result<T, E = Error> = std::result::Result<T, E>;

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::OpenDal { source } => opendal_error(*source),
            Error::Config { source } => BaseError::new(*source)
                .with_sub_category("config")
                .with_status(500)
                .with_exception(true),
            #[cfg(feature = "s3")]
            Error::ParseUri { source } => BaseError::new(*source)
                .with_sub_category("parse_uri")
                .with_status(500)
                .with_exception(true),
            Error::Validate { source } => BaseError::new(*source)
                .with_sub_category("validate")
                .with_status(500)
                .with_exception(true),
            Error::InvalidPath { message } => BaseError::new(message)
                .with_sub_category("invalid_path")
                .with_status(400)
                .with_exception(false),
            Error::Invalid { message } => BaseError::new(message)
                .with_sub_category("invalid")
                .with_status(500)
                .with_exception(true),
        };
        err.with_category("open_dal")
    }
}

/// 按 OpenDAL 的错误类别映射状态码。
fn opendal_error(source: opendal::Error) -> BaseError {
    use opendal::ErrorKind;
    match source.kind() {
        ErrorKind::NotFound => BaseError::new("object not found")
            .with_sub_category("not_found")
            .with_status(404)
            .with_exception(false),
        ErrorKind::PermissionDenied => BaseError::new("permission denied")
            .with_sub_category("permission_denied")
            .with_status(403)
            .with_exception(false),
        ErrorKind::AlreadyExists => BaseError::new("object already exists")
            .with_sub_category("already_exists")
            .with_status(409)
            .with_exception(false),
        ErrorKind::ConditionNotMatch => BaseError::new("precondition failed")
            .with_sub_category("condition_not_match")
            .with_status(412)
            .with_exception(false),
        ErrorKind::RangeNotSatisfied => BaseError::new("requested range not satisfiable")
            .with_sub_category("range_not_satisfied")
            .with_status(416)
            .with_exception(false),
        ErrorKind::IsADirectory | ErrorKind::NotADirectory => BaseError::new("invalid object path")
            .with_sub_category("invalid_path")
            .with_status(400)
            .with_exception(false),
        // 后端不支持该能力（如 fs 上 presign）：请求方选错了操作，不是故障
        ErrorKind::Unsupported => BaseError::new(source.to_string())
            .with_sub_category("unsupported")
            .with_status(501)
            .with_exception(false),
        ErrorKind::RateLimited => BaseError::new("storage backend rate limited")
            .with_sub_category("rate_limited")
            .with_status(429)
            .with_exception(false),
        ErrorKind::ConfigInvalid => BaseError::new("storage configuration invalid")
            .with_sub_category("config_invalid")
            .with_status(500)
            .with_exception(true),
        _ => BaseError::new(source)
            .with_sub_category("backend")
            .with_status(500)
            .with_exception(true),
    }
}

/// S3 连接参数，从 URL 查询字符串中解析。
#[cfg(feature = "s3")]
#[derive(Deserialize, PartialEq)]
struct S3Params {
    bucket: String,
    region: Option<String>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
}

#[cfg(feature = "s3")]
impl fmt::Debug for S3Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Params")
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("access_key_id", &self.access_key_id.as_ref().map(|_| "***"))
            .field(
                "secret_access_key",
                &self.secret_access_key.as_ref().map(|_| "***"),
            )
            .finish()
    }
}

/// 从 `s3://` / `http(s)://` URI 构造 S3 endpoint：若 scheme 为 `s3` 则转换为 `https`。
#[cfg(feature = "s3")]
fn s3_endpoint_from_uri(schema: &str, host: &str) -> String {
    if schema.eq_ignore_ascii_case("s3") {
        format!("https://{host}")
    } else {
        format!("{schema}://{host}")
    }
}

/// 将 OpenDAL builder 组装成统一的 `Storage`：
/// 1) 套 `MimeGuessLayer` 让对象自动带 Content-Type
/// 2) 错误统一走 `OpenDalSnafu`
#[cfg(any(feature = "fs", feature = "s3", feature = "http"))]
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
#[cfg(feature = "s3")]
fn new_s3_dal(url: &str) -> Result<Storage> {
    let parsed = parse_uri::<S3Params>(url).context(ParseUriSnafu)?;
    if parsed.hosts.len() != 1 {
        return Err(Error::Invalid {
            message: "s3 uri must specify exactly one host".to_string(),
        });
    }
    let endpoint = s3_endpoint_from_uri(parsed.schema, &parsed.hosts[0].to_authority());
    let mut builder = opendal::services::S3::default().endpoint(&endpoint);
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

#[cfg(feature = "fs")]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

/// 将路径字符串规范化为绝对路径，支持 `~/` 家目录前缀展开。
#[cfg(feature = "fs")]
#[inline]
fn resolve_path(path_str: &str) -> String {
    if path_str.is_empty() {
        return String::new();
    }
    let path = if let Some(stripped) = path_str.strip_prefix("~/") {
        home_dir()
            .map(|home| home.join(stripped))
            .unwrap_or_else(|| PathBuf::from(path_str))
    } else if path_str == "~" {
        home_dir().unwrap_or_else(|| PathBuf::from(path_str))
    } else {
        PathBuf::from(path_str)
    };

    std::path::absolute(&path).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |p| p.to_string_lossy().into_owned(),
    )
}

/// 从 `file://` URL 创建本地文件系统存储后端，根路径需至少 2 个字符。
#[cfg(feature = "fs")]
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
#[cfg(feature = "http")]
fn new_http_dal(url: &str) -> Result<Storage> {
    finalize_dal(opendal::services::Http::default().endpoint(url))
}

/// 根据配置 URL 自动选择存储后端并创建 Storage 实例。
/// - `file://` → 本地文件系统后端（需启用 `fs` feature，默认启用）
/// - `schema = "http"` → HTTP 只读后端（需启用 `http` feature）
/// - 其余 → S3 兼容后端（需启用 `s3` feature）
pub fn new_opendal_storage(config: &Config) -> Result<Storage> {
    let opendal_config = new_opendal_config(config)?;
    let url = opendal_config.url.as_str();
    new_opendal_storage_from_url(url, Some(&opendal_config.schema))
}

/// 根据 URL 和 schema 自动选择存储后端并创建 Storage 实例。
pub fn new_opendal_storage_from_url(url: &str, schema: Option<&str>) -> Result<Storage> {
    if url.starts_with("file://") {
        #[cfg(feature = "fs")]
        {
            new_fs_dal(url)
        }
        #[cfg(not(feature = "fs"))]
        {
            Err(Error::Invalid {
                message: "local filesystem storage requires `tibba-opendal/fs` feature".to_string(),
            })
        }
    } else if schema == Some("http") {
        #[cfg(feature = "http")]
        {
            new_http_dal(url)
        }
        #[cfg(not(feature = "http"))]
        {
            Err(Error::Invalid {
                message: "http storage requires `tibba-opendal/http` feature".to_string(),
            })
        }
    } else {
        #[cfg(feature = "s3")]
        {
            new_s3_dal(url)
        }
        #[cfg(not(feature = "s3"))]
        {
            let _ = url;
            Err(Error::Invalid {
                message: "s3 storage requires `tibba-opendal/s3` feature".to_string(),
            })
        }
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

        assert_eq!(
            opendal_error(opendal::Error::new(ErrorKind::PermissionDenied, "denied")).status(),
            403
        );
        assert_eq!(
            opendal_error(opendal::Error::new(ErrorKind::AlreadyExists, "exists")).status(),
            409
        );
        assert_eq!(
            opendal_error(opendal::Error::new(ErrorKind::ConditionNotMatch, "etag")).status(),
            412
        );
        assert_eq!(
            opendal_error(opendal::Error::new(ErrorKind::RangeNotSatisfied, "range")).status(),
            416
        );
        assert_eq!(
            opendal_error(opendal::Error::new(ErrorKind::IsADirectory, "dir")).status(),
            400
        );
        let rate = opendal_error(opendal::Error::new(ErrorKind::RateLimited, "slow"));
        assert_eq!(rate.status(), 429);
        assert!(!rate.is_exception());
    }

    #[test]
    fn validate_object_path_rejects_traversal_and_control_chars() {
        assert_eq!(
            validate_object_path("2026/07/a.png").expect("valid"),
            "2026/07/a.png"
        );
        assert_eq!(
            validate_object_path("/2026/07/a.png").expect("valid"),
            "2026/07/a.png"
        );
        for bad in [
            "",
            "/",
            "dir/",
            "../etc/passwd",
            "a/../b",
            "./a",
            "a/./b",
            "a//b",
            "a\\b",
            "a\0b",
            "a\nb",
        ] {
            let err = validate_object_path(bad).expect_err(bad);
            let base = BaseError::from(err);
            assert_eq!(base.status(), 400);
            assert!(!base.is_exception());
        }
    }

    #[test]
    fn redact_url_preserves_at_in_path_and_strips_fragment() {
        assert_eq!(
            redact_url("https://s3.example.com/user@org/obj.png?X-Amz-Signature=xyz#frag"),
            "https://s3.example.com/user@org/obj.png?<redacted>"
        );
    }

    #[test]
    fn presign_debug_redacts_secrets() {
        let presign = PresignResult {
            method: "PUT".to_string(),
            url: "https://s3.example.com/a.png?X-Amz-Signature=secret999".to_string(),
            headers: vec![
                ("content-type".to_string(), "image/png".to_string()),
                ("authorization".to_string(), "Bearer topsecret".to_string()),
            ],
        };
        let p_dbg = format!("{presign:?}");
        assert!(!p_dbg.contains("secret999") && !p_dbg.contains("topsecret"));
        assert!(p_dbg.contains("image/png"));
    }

    #[cfg(feature = "s3")]
    #[test]
    fn s3_debug_and_endpoint_conversion_work() {
        let s3 = S3Params {
            bucket: "b".to_string(),
            region: Some("us-east-1".to_string()),
            access_key_id: Some("AKIA123456".to_string()),
            secret_access_key: Some("SUPERSECRET".to_string()),
        };
        let dbg = format!("{s3:?}");
        assert!(!dbg.contains("AKIA123456") && !dbg.contains("SUPERSECRET"));
        assert_eq!(
            s3_endpoint_from_uri("s3", "s3.us-east-1.amazonaws.com"),
            "https://s3.us-east-1.amazonaws.com"
        );
        assert_eq!(
            s3_endpoint_from_uri("http", "127.0.0.1:9000"),
            "http://127.0.0.1:9000"
        );
    }

    #[cfg(not(feature = "s3"))]
    #[test]
    fn s3_url_returns_feature_error_when_s3_disabled() {
        let err = new_opendal_storage_from_url("s3://s3.example.com?bucket=b", None)
            .expect_err("s3 未启用应报错");
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
    }
}
