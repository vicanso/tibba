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

use super::{DeserializeSnafu, Error};
use serde::de::DeserializeOwned;
use snafu::ResultExt;
use url::Url;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ParsedUri<'a, Q> {
    pub origin_uri: &'a str,
    pub schema: &'a str,
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
    pub hosts: Vec<Host<'a>>,
    pub path: Option<&'a str>,
    pub raw_query: Option<&'a str>,
    pub query: Q,
}

impl<'a, Q> ParsedUri<'a, Q> {
    /// 各 host 的 `host:port` 文本形式；无端口时只有主机名。
    /// IPv6 会补回方括号，结果可直接拼进 URL。
    pub fn host_strings(&self) -> Vec<String> {
        self.hosts.iter().map(Host::to_authority).collect()
    }
    pub fn endpoint(&self) -> String {
        match self.hosts.first() {
            Some(host) => format!("{}://{}", self.schema, host.to_authority()),
            None => String::new(),
        }
    }
    /// 重建指向**第一个** host 的 URL（多 host 场景取首个节点）。
    pub fn url(&self) -> Result<Url> {
        // 无 host：返回错误而非 panic（此前 else 分支 arr[0] 会越界，畸形 uri 致启动崩溃）
        let host = self.hosts.first().ok_or(Error::Invalid {
            message: "uri has no host".to_string(),
        })?;

        // 从解析出的各段重新拼装，而不是在 origin_uri 上做字符串替换。
        // 此前是 `origin_uri.replace(&hosts.join(","), &arr[0])`：`replace` 会命中
        // **所有**匹配位置，host 串若恰好也出现在 path 或 query 里（如
        // `postgres://h1,h2/db?fallback=h1,h2`），就会把那里一并改掉。
        let mut buf = String::with_capacity(self.origin_uri.len());
        buf.push_str(self.schema);
        buf.push_str("://");
        if let Some(user) = self.username {
            buf.push_str(user);
            if let Some(pass) = self.password {
                buf.push(':');
                buf.push_str(pass);
            }
            buf.push('@');
        }
        buf.push_str(&host.to_authority());
        if let Some(path) = self.path {
            buf.push('/');
            buf.push_str(path);
        }
        if let Some(query) = self.raw_query {
            buf.push('?');
            buf.push_str(query);
        }

        Url::parse(&buf).map_err(|e| Error::Invalid {
            message: e.to_string(),
        })
    }
}

/// 单个主机节点。
///
/// `name` 存**不含方括号**的裸地址，因此 IPv6 形如 `::1` 而非 `[::1]`——这样可以
/// 直接交给 `IpAddr::from_str` / `lookup_host`。需要 URL 文本形式时用
/// [`Host::to_authority`]，它会按需补回方括号。
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Host<'a> {
    pub name: &'a str,
    pub port: Option<u16>,
}

impl Host<'_> {
    /// URL authority 文本形式：`host`、`host:port`、`[v6]` 或 `[v6]:port`。
    pub fn to_authority(&self) -> String {
        // 裸地址含冒号即 IPv6，拼进 URL 时必须加方括号，否则无法与端口分隔符区分
        let is_ipv6 = self.name.contains(':');
        match (is_ipv6, self.port) {
            (true, Some(port)) => format!("[{}]:{}", self.name, port),
            (true, None) => format!("[{}]", self.name),
            (false, Some(port)) => format!("{}:{}", self.name, port),
            (false, None) => self.name.to_string(),
        }
    }
}

/// 解析端口号。
fn parse_port(value: &str) -> Result<u16> {
    value.parse::<u16>().map_err(|e| Error::Invalid {
        message: format!("invalid port {value:?}: {e}"),
    })
}

/// 解析单个 host 片段，支持 `host`、`host:port`、`[v6]`、`[v6]:port` 四种形式。
fn parse_host(part: &str) -> Result<Host<'_>> {
    // RFC 3986：IPv6 字面量必须用方括号包裹，否则地址自身的冒号与端口分隔符无法区分
    if let Some(rest) = part.strip_prefix('[') {
        let (addr, tail) = rest.split_once(']').ok_or(Error::Invalid {
            message: format!("unclosed IPv6 bracket in host {part:?}"),
        })?;
        let port = match tail {
            "" => None,
            _ => {
                let digits = tail.strip_prefix(':').ok_or(Error::Invalid {
                    message: format!("unexpected text after IPv6 bracket in host {part:?}"),
                })?;
                Some(parse_port(digits)?)
            }
        };
        return Ok(Host { name: addr, port });
    }

    // 未加括号却有多个冒号：几乎必然是漏写方括号的 IPv6。若按 rsplit_once 处理，
    // `::1` 会被静默拆成 name="::" / port=1 —— 报错好过悄悄连到错误的地址。
    if part.matches(':').count() > 1 {
        return Err(Error::Invalid {
            message: format!("IPv6 address must be bracketed: [{part}]"),
        });
    }

    match part.rsplit_once(':') {
        Some((name, port_str)) => Ok(Host {
            name,
            port: Some(parse_port(port_str)?),
        }),
        None => Ok(Host {
            name: part,
            port: None,
        }),
    }
}

pub fn parse_uri<'a, Q>(uri: &'a str) -> Result<ParsedUri<'a, Q>>
where
    Q: DeserializeOwned,
{
    let (schema, rest) = uri.split_once("://").ok_or(Error::Invalid {
        message: "Missing scheme".to_string(),
    })?;
    let (main, query_str) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = main.split_once('/').unwrap_or((main, ""));
    let path = if path.is_empty() { None } else { Some(path) };
    // 按**最后一个** `@` 切分 userinfo 与 host。RFC 3986 要求 userinfo 里的 `@`
    // 转义，但现实中的口令经常直接塞原文；取第一个 `@` 会把
    // `redis://user:p@ss@host:6379` 解析成 password="p" / host="ss@host:6379"——
    // 口令和主机同时错，且不会报任何错。同 crate 的 `redact_node_url`
    // （tibba-cache）早已用 rsplit，这里此前与它不一致。
    let (user_info, hosts_str) = authority.rsplit_once('@').unwrap_or(("", authority));
    let (username, password) = if user_info.is_empty() {
        (None, None)
    } else {
        let (user, pass) = user_info.split_once(':').unwrap_or((user_info, ""));
        (Some(user), if pass.is_empty() { None } else { Some(pass) })
    };
    if hosts_str.is_empty() {
        return Err(Error::Invalid {
            message: "Missing hosts".to_string(),
        });
    }
    let hosts: Result<Vec<Host>> = hosts_str
        .split(',')
        .filter(|s| !s.is_empty())
        .map(parse_host)
        .collect();

    let query: Q = serde_urlencoded::from_str(query_str).context(DeserializeSnafu)?;

    let raw_query = if query_str.is_empty() {
        None
    } else {
        Some(query_str)
    };
    let hosts = hosts?;
    // 上面的 `hosts_str.is_empty()` 只挡住了「@ 之后完全没有内容」，挡不住全是分隔符
    // 的情形：`redis://,` 会被 filter 掉所有空片段，剩下零个 host 却仍然解析成功，
    // 之后 `endpoint()` 静默返回空串、连接配置里一个节点都没有。此处补上兜底，
    // 使「hosts 非空」成为 ParsedUri 的不变量。
    if hosts.is_empty() {
        return Err(Error::Invalid {
            message: "Missing hosts".to_string(),
        });
    }
    Ok(ParsedUri {
        origin_uri: uri,
        schema,
        username,
        password,
        raw_query,
        hosts,
        path,
        query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestQuery {
        #[serde(rename = "replicaSet")]
        replica_set: Option<String>,
        #[serde(default)]
        timeout: u32,
    }

    #[test]
    fn test_deserialize_to_struct() {
        let uri = "mongodb://user@node1:27017/db?replicaSet=rs0&timeout=5";

        let parsed = parse_uri::<TestQuery>(uri).unwrap();

        assert_eq!(parsed.schema, "mongodb");
        assert_eq!(parsed.username, Some("user"));
        assert_eq!(
            parsed.hosts,
            vec![Host {
                name: "node1",
                port: Some(27017)
            }]
        );
        assert_eq!(parsed.path, Some("db"));

        assert_eq!(parsed.query.replica_set, Some("rs0".to_string()));
        assert_eq!(parsed.query.timeout, 5);
    }

    #[test]
    fn test_deserialize_to_hashmap() {
        let uri = "kafka://broker:9092?client.id=app-1&retries=3";
        let parsed = parse_uri::<HashMap<String, String>>(uri).unwrap();

        assert_eq!(parsed.query.get("client.id"), Some(&"app-1".to_string()));
        assert_eq!(parsed.query.get("retries"), Some(&"3".to_string()));
    }

    #[test]
    fn test_deserialization_error() {
        // wrong timeout
        let uri = "schema://host?timeout=five";
        let err = parse_uri::<TestQuery>(uri).unwrap_err();

        assert_eq!(err.to_string(), "invalid digit found in string");
    }

    /// IPv6 字面量：带端口、不带端口都要能解析。
    /// 旧实现对 `[::1]` 会把 `[:` 当 host、`1]` 当端口，报「invalid digit」，
    /// 直接导致 IPv6 部署起不来。
    #[test]
    fn ipv6_literal_with_and_without_port() {
        let parsed = parse_uri::<HashMap<String, String>>("redis://[::1]:6379").unwrap();
        assert_eq!(
            parsed.hosts,
            vec![Host {
                name: "::1", // name 不含方括号，可直接喂给 IpAddr::from_str
                port: Some(6379),
            }]
        );
        // 拼回 URL 文本时方括号必须补上
        assert_eq!(parsed.host_strings(), vec!["[::1]:6379".to_string()]);
        assert_eq!(parsed.endpoint(), "redis://[::1]:6379");

        let parsed = parse_uri::<HashMap<String, String>>("redis://[fe80::1ff:fe23:4567:890a]")
            .expect("无端口的 IPv6 也必须能解析");
        assert_eq!(
            parsed.hosts,
            vec![Host {
                name: "fe80::1ff:fe23:4567:890a",
                port: None,
            }]
        );
        assert_eq!(
            parsed.host_strings(),
            vec!["[fe80::1ff:fe23:4567:890a]".to_string()]
        );
    }

    #[test]
    fn ipv6_with_userinfo_path_and_query() {
        let parsed =
            parse_uri::<HashMap<String, String>>("postgres://user:pw@[::1]:5432/mydb?sslmode=require")
                .unwrap();
        assert_eq!(parsed.username, Some("user"));
        assert_eq!(parsed.password, Some("pw"));
        assert_eq!(parsed.hosts, vec![Host { name: "::1", port: Some(5432) }]);
        assert_eq!(parsed.path, Some("mydb"));
        assert_eq!(
            parsed.url().unwrap().as_str(),
            "postgres://user:pw@[::1]:5432/mydb?sslmode=require"
        );
    }

    /// 多 host 集群里混用 IPv4 / IPv6。
    #[test]
    fn mixed_ipv4_and_ipv6_hosts() {
        let parsed =
            parse_uri::<HashMap<String, String>>("redis://10.0.0.1:6379,[::1]:6380,node3").unwrap();
        assert_eq!(
            parsed.host_strings(),
            vec![
                "10.0.0.1:6379".to_string(),
                "[::1]:6380".to_string(),
                "node3".to_string(),
            ]
        );
    }

    /// 漏写方括号的 IPv6 必须报错，而不是被 rsplit_once 静默拆成
    /// name="::" / port=1 —— 那会连到一个完全不同的地址。
    #[test]
    fn unbracketed_ipv6_is_rejected_not_silently_misparsed() {
        let err = parse_uri::<HashMap<String, String>>("redis://::1:6379").unwrap_err();
        assert!(
            err.to_string().contains("must be bracketed"),
            "错误信息应提示补方括号，实际: {err}"
        );
        // 方括号未闭合
        assert!(parse_uri::<HashMap<String, String>>("redis://[::1:6379").is_err());
    }

    /// `url()` 从各段重建，不能像旧实现那样在整串上做 `replace`：
    /// host 列表若恰好也出现在 query 里，replace 会把那里一并改掉。
    #[test]
    fn url_rebuild_does_not_corrupt_query_containing_host_text() {
        let uri = "postgres://h1:5432,h2:5432/mydb?fallback=h1:5432,h2:5432";
        let parsed = parse_uri::<HashMap<String, String>>(uri).unwrap();
        let url = parsed.url().unwrap();

        // 只取首个节点作为连接目标
        assert_eq!(url.host_str(), Some("h1"));
        assert_eq!(url.port(), Some(5432));
        assert_eq!(url.path(), "/mydb");
        // 关键：query 原样保留，没有被 replace 误伤
        assert_eq!(url.query(), Some("fallback=h1:5432,h2:5432"));
    }

    #[test]
    fn url_single_host_round_trips() {
        let parsed =
            parse_uri::<HashMap<String, String>>("postgres://u:p@db.internal:5432/app").unwrap();
        assert_eq!(
            parsed.url().unwrap().as_str(),
            "postgres://u:p@db.internal:5432/app"
        );
    }

    /// **回归守卫**：口令含未转义 `@` 时，必须按最后一个 `@` 切分。
    ///
    /// 旧实现用 `split_once`（第一个 `@`），于是
    /// `redis://user:p@ss@host:6379` 被解析成 password=`p`、host=`ss@host:6379`——
    /// 主机名整个是错的，`endpoint()` / `host_strings()` 会把连接指向一个不存在
    /// 的地址（`tibba-opendal` 的 S3 endpoint 正是这么取的），且全程没有报错。
    #[test]
    fn unescaped_at_in_password_splits_on_last_separator() {
        let parsed = parse_uri::<HashMap<String, String>>("redis://user:p@ss@host:6379").unwrap();
        assert_eq!(parsed.username, Some("user"));
        assert_eq!(parsed.password, Some("p@ss"));
        assert_eq!(parsed.host_strings(), vec!["host:6379".to_string()]);
        assert_eq!(parsed.endpoint(), "redis://host:6379");

        // 无用户名、仅口令的形式（redis 常见写法）
        let parsed = parse_uri::<HashMap<String, String>>("redis://:p@ss@host:6379").unwrap();
        assert_eq!(parsed.username, Some(""));
        assert_eq!(parsed.password, Some("p@ss"));
        assert_eq!(parsed.host_strings(), vec!["host:6379".to_string()]);

        // 重建 URL 时也要还原得回去
        let parsed =
            parse_uri::<HashMap<String, String>>("postgres://u:pa@ss@db.internal:5432/app").unwrap();
        assert_eq!(parsed.hosts, vec![Host { name: "db.internal", port: Some(5432) }]);
        let url = parsed.url().unwrap();
        assert_eq!(url.host_str(), Some("db.internal"));
        assert_eq!(url.port(), Some(5432));
    }

    /// 无 userinfo 的 URI 不能被 rsplit 改坏。
    #[test]
    fn no_userinfo_is_unaffected() {
        let parsed = parse_uri::<HashMap<String, String>>("redis://host:6379").unwrap();
        assert_eq!(parsed.username, None);
        assert_eq!(parsed.password, None);
        assert_eq!(parsed.host_strings(), vec!["host:6379".to_string()]);
    }

    /// 全是分隔符的 authority 必须报错。
    ///
    /// 旧实现只检查 `hosts_str` 非空，`","` 能过这一关，随后所有空片段被 filter 掉，
    /// 得到一个 hosts 为空的 ParsedUri —— `endpoint()` 会静默返回空串，
    /// 配置里一个节点都没有却没有任何报错。
    #[test]
    fn authority_with_only_separators_is_rejected() {
        for uri in ["redis://,", "redis://,,", "redis://,,,/db"] {
            let err = parse_uri::<HashMap<String, String>>(uri)
                .expect_err("{uri} 应当被拒绝，而不是解析出零个 host");
            assert!(
                err.to_string().contains("Missing hosts"),
                "错误信息应指出缺少 host，实际: {err}"
            );
        }
    }
}
