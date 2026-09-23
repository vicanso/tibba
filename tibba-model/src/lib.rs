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

use chrono::{DateTime, offset};
use snafu::Snafu;
use tibba_error::Error as BaseError;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime};

pub fn format_datetime(datetime: PrimitiveDateTime) -> String {
    let ts = datetime.assume_utc().unix_timestamp();
    if let Some(value) = DateTime::from_timestamp(ts, 0) {
        value.with_timezone(&offset::Local).to_string()
    } else {
        String::new()
    }
}

/// 响应头是否允许由**数据**（文件元数据、按分组配置的响应头）设置。
///
/// 文件下载 / 预览会把库里的 `files.metadata` 与 `configurations` 中的响应头合并进
/// 响应。此前不做任何过滤，而合并发生在安全相关头之后、且 `HeaderMap::extend` 对
/// 同名头是替换语义，于是：
///
/// - 写一条 `Content-Type: text/html` + `Content-Disposition: inline`，任何人下载该
///   文件都会在**应用同源**下渲染攻击者的 HTML——存储型 XSS；
/// - `configurations` 的写权限（`model:configuration:write`）低于 Admin，持有它的人
///   借此就能在超管浏览器里执行脚本、拿走超管会话，是一条提权路径；
/// - `Set-Cookie` 还能直接给访问者种 cookie（会话固定）。
///
/// 规则是拒绝名单而非白名单：缓存控制、自定义 `x-*` 等头有正当用途，但凡是
/// 决定**内容如何被解释**、**安全策略**、**连接与跳转**的头一律不许数据改写。
#[must_use]
pub fn is_data_overridable_response_header(name: &http::HeaderName) -> bool {
    const DENY: &[&str] = &[
        // 内容如何被解释
        "content-type",
        "content-disposition",
        "content-encoding",
        "content-length",
        "content-range",
        "x-content-type-options",
        // 会话与跳转
        "set-cookie",
        "location",
        "refresh",
        // 安全策略
        "content-security-policy",
        "content-security-policy-report-only",
        "strict-transport-security",
        "x-frame-options",
        "x-xss-protection",
        "referrer-policy",
        "permissions-policy",
        "cross-origin-opener-policy",
        "cross-origin-embedder-policy",
        "cross-origin-resource-policy",
        // 连接层（hop-by-hop），由服务端自己管理
        "connection",
        "keep-alive",
        "transfer-encoding",
        "upgrade",
        "trailer",
        "te",
    ];
    let name = name.as_str();
    !(DENY.contains(&name) || name.starts_with("access-control-") || name.starts_with("proxy-"))
}

/// 返回当前 UTC 时刻的 `PrimitiveDateTime`，用于与 SQL 端 timestamp 类型比较。
/// 之前 `get_response_headers` / `get_config` 各写了一份相同的构造逻辑，统一在此。
pub fn now_primitive_utc() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

pub fn parse_primitive_datetime(s: &str) -> Result<PrimitiveDateTime> {
    let fmt_t = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    let fmt_space = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    if let Ok(dt) = PrimitiveDateTime::parse(s, fmt_t) {
        return Ok(dt);
    }
    if let Ok(dt) = PrimitiveDateTime::parse(s, fmt_space) {
        return Ok(dt);
    }
    if let Ok(odt) = OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        let utc = odt.to_offset(time::UtcOffset::UTC);
        return Ok(PrimitiveDateTime::new(utc.date(), utc.time()));
    }
    Err(Error::InvalidDatetime {
        value: s.to_string(),
    })
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    #[snafu(display("{source}"))]
    Sqlx { source: sqlx::Error },
    /// 客户端传入的 JSON（insert / update 参数、列表过滤条件）解析失败 → 400。
    #[snafu(display("{source}"))]
    Json { source: serde_json::Error },
    /// **库里存的** JSON 解析失败 → 500。与 [`Error::Json`] 分开：前者是调用方
    /// 写错了字段，后者是数据本身坏了（或 schema 改了没迁移），处理方式截然不同。
    #[snafu(display("decode stored json: {source}"))]
    StoredJson { source: serde_json::Error },
    #[snafu(display("Not supported function: {}", name))]
    NotSupported { name: String },
    #[snafu(display("Not found"))]
    NotFound,
    #[snafu(display("Invalid datetime: {value}"))]
    InvalidDatetime { value: String },
    #[snafu(display("Insufficient balance"))]
    InsufficientBalance,
    #[snafu(display("{message}"))]
    InvalidAmount { message: String },
    #[snafu(display("{source}"))]
    Crypto { source: tibba_crypto::Error },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::Sqlx { source } => sqlx_error(source),
            // 除 StoredJson 外的 JSON 错误都来自客户端参数：字段写错是 400，
            // 不是服务端故障，更不该半夜告警
            Error::Json { source } => BaseError::new(source)
                .with_sub_category("json")
                .with_status(400),
            Error::StoredJson { source } => BaseError::new(source)
                .with_sub_category("stored_json")
                .with_status(500)
                .with_exception(true),
            // 通用 model 路由把 CRUD 暴露给客户端，某个 model 不支持某操作是
            // 请求方的问题：405，而非「500 + 告警」
            Error::NotSupported { name } => {
                BaseError::new(format!("Not supported function: {name}"))
                    .with_sub_category("not_supported")
                    .with_status(405)
            }
            Error::NotFound => BaseError::new("Not found")
                .with_sub_category("not_found")
                .with_status(404),
            // 时间参数来自查询串，格式错是客户端错误（此前未设状态码 → 500）
            Error::InvalidDatetime { value } => {
                BaseError::new(format!("Invalid datetime: {value}"))
                    .with_sub_category("invalid_datetime")
                    .with_status(400)
            }
            Error::InsufficientBalance => BaseError::new("Insufficient balance")
                .with_sub_category("insufficient_balance")
                .with_status(402),
            Error::InvalidAmount { message } => BaseError::new(message)
                .with_sub_category("invalid_amount")
                .with_status(400),
            // 复用 tibba_crypto 的转换（保留 argon2 sub_category / 状态），外层再归类到 model
            Error::Crypto { source } => BaseError::from(source),
        };
        err.with_category("model")
    }
}

/// 把 sqlx 错误按**成因**映射成状态码。
///
/// 此前一律 `500 + exception`，于是：
/// - 重复注册账号（`users.account` 唯一索引冲突）回的是
///   `500 {"message":"internal server error"}` 并触发告警；
/// - 按 ID 查不存在的行（`RowNotFound`）同样被当成服务端故障。
///
/// 约束类错误是**请求与现有数据冲突**，属于客户端可理解、可处理的结果：
///
/// | 成因 | 状态码 |
/// |------|--------|
/// | `RowNotFound` | 404 |
/// | 唯一约束冲突 | 409 |
/// | 外键冲突（引用的行不存在 / 仍被引用） | 409 |
/// | 非空 / CHECK 约束 | 400 |
/// | 其余（连接、超时、语法…） | 500 + 告警 |
///
/// 4xx 的 message 用固定文案而不是 Postgres 原文：原文会带出表结构细节。
/// 约束名放进 extra——它对前端判断「哪个字段冲突了」有用，又不含数据本身。
fn sqlx_error(source: sqlx::Error) -> BaseError {
    use sqlx::error::ErrorKind;

    let client = |status: u16, sub: &str, message: &str, constraint: Option<&str>| {
        let err = BaseError::new(message)
            .with_sub_category(sub)
            .with_status(status);
        match constraint {
            Some(c) => err.add_extra(format!("constraint={c}")),
            None => err,
        }
    };

    if matches!(source, sqlx::Error::RowNotFound) {
        return client(404, "not_found", "Not found", None);
    }
    if let sqlx::Error::Database(db) = &source {
        let constraint = db.constraint();
        match db.kind() {
            ErrorKind::UniqueViolation => {
                return client(
                    409,
                    "unique_violation",
                    "resource already exists",
                    constraint,
                );
            }
            ErrorKind::ForeignKeyViolation => {
                return client(
                    409,
                    "foreign_key_violation",
                    "referenced resource conflict",
                    constraint,
                );
            }
            ErrorKind::NotNullViolation | ErrorKind::CheckViolation => {
                return client(
                    400,
                    "constraint_violation",
                    "invalid field value",
                    constraint,
                );
            }
            _ => {}
        }
    }
    BaseError::new(source)
        .with_sub_category("sqlx")
        .with_status(500)
        .with_exception(true)
}

mod configuration;
mod model;
mod schema;
mod user;

pub use configuration::*;
pub use model::*;
pub use schema::*;
pub use user::*;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use time::macros::datetime;

    #[test]
    fn parse_t_separator_format() {
        let dt = parse_primitive_datetime("2026-06-05T12:34:56").unwrap();
        assert_eq!(dt, datetime!(2026-06-05 12:34:56));
    }

    #[test]
    fn parse_space_separator_format() {
        let dt = parse_primitive_datetime("2026-06-05 12:34:56").unwrap();
        assert_eq!(dt, datetime!(2026-06-05 12:34:56));
    }

    #[test]
    fn parse_rfc3339_converts_to_utc() {
        // +08:00 时刻 → UTC 应减 8 小时
        let dt = parse_primitive_datetime("2026-06-05T12:34:56+08:00").unwrap();
        assert_eq!(dt, datetime!(2026-06-05 04:34:56));
    }

    #[test]
    fn parse_invalid_returns_error() {
        let err = parse_primitive_datetime("not a date").unwrap_err();
        assert!(matches!(err, Error::InvalidDatetime { ref value } if value == "not a date"));
    }

    #[test]
    fn now_primitive_utc_matches_system_clock() {
        // 与 std 系统时钟比较，验证「现在」差距在 1 秒内（CI 慢机也够用）
        let ours = now_primitive_utc();
        let system_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_secs();
        let system_ts = i64::try_from(system_secs).expect("timestamp fits in i64");
        let ours_ts = ours.assume_utc().unix_timestamp();
        let diff = (ours_ts - system_ts).abs();
        assert!(
            diff <= 1,
            "now_primitive_utc 应与系统时钟一致，差距 {diff}s"
        );
    }
}

#[cfg(test)]
mod error_mapping_tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use sqlx::error::{DatabaseError, ErrorKind};
    use std::borrow::Cow;

    /// 最小化的数据库错误桩：只需要 kind 与约束名。
    #[derive(Debug)]
    struct FakeDbError {
        kind: ErrorKind,
        constraint: Option<&'static str>,
    }

    impl std::fmt::Display for FakeDbError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("duplicate key value violates unique constraint \"user_account\"")
        }
    }

    impl std::error::Error for FakeDbError {}

    impl DatabaseError for FakeDbError {
        fn message(&self) -> &str {
            "duplicate key value violates unique constraint"
        }
        fn code(&self) -> Option<Cow<'_, str>> {
            None
        }
        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
        fn constraint(&self) -> Option<&str> {
            self.constraint
        }
        fn kind(&self) -> ErrorKind {
            match self.kind {
                ErrorKind::UniqueViolation => ErrorKind::UniqueViolation,
                ErrorKind::ForeignKeyViolation => ErrorKind::ForeignKeyViolation,
                ErrorKind::NotNullViolation => ErrorKind::NotNullViolation,
                ErrorKind::CheckViolation => ErrorKind::CheckViolation,
                _ => ErrorKind::Other,
            }
        }
    }

    fn db(kind: ErrorKind, constraint: Option<&'static str>) -> BaseError {
        BaseError::from(Error::Sqlx {
            source: sqlx::Error::Database(Box::new(FakeDbError { kind, constraint })),
        })
    }

    /// **回归守卫**：重复注册账号是 409，而不是「500 + 告警」。
    #[test]
    fn unique_violation_is_conflict() {
        let err = db(ErrorKind::UniqueViolation, Some("user_account"));
        assert_eq!(err.status(), 409);
        assert!(!err.is_exception(), "约束冲突不是服务端故障，不应告警");
        assert_eq!(err.sub_category(), Some("unique_violation"));
        // 约束名进 extra，供前端判断是哪个字段冲突
        assert_eq!(err.extra(), ["constraint=user_account".to_string()]);
        // 对外 message 不能带出 Postgres 原文（含表结构细节）
        assert!(!err.message().contains("duplicate key"));
    }

    #[test]
    fn constraint_violations_map_to_client_errors() {
        assert_eq!(db(ErrorKind::ForeignKeyViolation, None).status(), 409);
        assert_eq!(db(ErrorKind::NotNullViolation, None).status(), 400);
        assert_eq!(
            db(ErrorKind::CheckViolation, Some("ck_amount")).status(),
            400
        );
    }

    /// 其余数据库错误仍是服务端故障：500 + 告警。
    #[test]
    fn other_database_errors_stay_server_errors() {
        let err = db(ErrorKind::Other, None);
        assert_eq!(err.status(), 500);
        assert!(err.is_exception());
    }

    #[test]
    fn protected_response_headers_cannot_be_set_by_data() {
        use http::HeaderName;
        let ok = |n: &'static str| is_data_overridable_response_header(&HeaderName::from_static(n));
        for denied in [
            "content-type",
            "content-disposition",
            "set-cookie",
            "location",
            "content-security-policy",
            "x-frame-options",
            "access-control-allow-origin",
            "transfer-encoding",
        ] {
            assert!(!ok(denied), "{denied} 不得由数据覆盖");
        }
        for allowed in [
            "cache-control",
            "expires",
            "etag",
            "content-language",
            "x-custom-tag",
        ] {
            assert!(ok(allowed), "{allowed} 应允许");
        }
    }

    #[test]
    fn row_not_found_is_404() {
        let err = BaseError::from(Error::Sqlx {
            source: sqlx::Error::RowNotFound,
        });
        assert_eq!(err.status(), 404);
        assert!(!err.is_exception());
    }

    /// 客户端参数的 JSON 错误是 400；库里存的数据坏了才是 500。
    #[test]
    fn client_json_vs_stored_json() {
        let bad = || serde_json::from_str::<u8>("x").expect_err("构造 JSON 错误");
        let client = BaseError::from(Error::Json { source: bad() });
        assert_eq!(client.status(), 400);
        assert!(!client.is_exception());

        let stored = BaseError::from(Error::StoredJson { source: bad() });
        assert_eq!(stored.status(), 500);
        assert!(stored.is_exception());
    }

    #[test]
    fn not_found_not_supported_and_datetime_status() {
        assert_eq!(BaseError::from(Error::NotFound).status(), 404);
        assert_eq!(
            BaseError::from(Error::NotSupported {
                name: "insert".to_string()
            })
            .status(),
            405
        );
        assert_eq!(
            BaseError::from(Error::InvalidDatetime {
                value: "x".to_string()
            })
            .status(),
            400
        );
    }
}
