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

use super::Error;
use serde::de::DeserializeOwned;
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
    /// Format hosts to "host:port" format string vector (Vec<String>).
    /// If the host does not specify a port, the result string only contains the host name.
    ///
    /// # Returns
    /// A Vec<String> containing the formatted "host:port" strings.
    pub fn host_strings(&self) -> Vec<String> {
        self.hosts
            .iter()
            .map(|host| {
                match host.port {
                    // if the host specifies a port, format it as "name:port"
                    Some(port) => format!("{}:{}", host.name, port),
                    None => host.name.to_string(),
                }
            })
            .collect()
    }
    pub fn endpoint(&self) -> String {
        if self.hosts.is_empty() {
            return String::new();
        }
        format!("{}://{}", self.schema, self.host_strings()[0])
    }
    pub fn url(&self) -> Result<Url> {
        let url = if self.hosts.len() == 1 {
            self.origin_uri.to_string()
        } else {
            let arr = self.host_strings();
            let hosts = arr.join(",");
            self.origin_uri.replace(&hosts, &arr[0]).to_string()
        };

        Url::parse(&url).map_err(|e| Error::Invalid {
            message: e.to_string(),
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Host<'a> {
    pub name: &'a str,
    pub port: Option<u16>,
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
    let (user_info, hosts_str) = authority.split_once('@').unwrap_or(("", authority));
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
        .map(|host_part| match host_part.rsplit_once(':') {
            Some((name, port_str)) => {
                let port = port_str.parse::<u16>().map_err(|e| Error::Invalid {
                    message: e.to_string(),
                })?;
                Ok(Host {
                    name,
                    port: Some(port),
                })
            }
            None => Ok(Host {
                name: host_part,
                port: None,
            }),
        })
        .collect();

    let query: Q =
        serde_urlencoded::from_str(query_str).map_err(|e| Error::Deserialize { source: e })?;

    let raw_query = if query_str.is_empty() {
        None
    } else {
        Some(query_str)
    };
    Ok(ParsedUri {
        origin_uri: uri,
        schema,
        username,
        password,
        raw_query,
        hosts: hosts?,
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
}
