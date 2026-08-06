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

use super::{
    CODE_FILE_GROUP, CODE_FILE_NAME, CODE_IMAGE_FORMAT, CODE_IMAGE_QUALITY, CODE_LISTEN_ADDR,
    CODE_SCHEMA_NAME, CODE_SHA256, CODE_UUID,
};
use super::{is_disabled, new_error, validate_ascii_name};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use uuid::Uuid;
use validator::ValidationError;

type Result<T> = std::result::Result<T, ValidationError>;

/// 端口 0 表示「由内核分配」，对显式配置的监听地址而言必定是配置错误。
fn check_listen_port(port: u16) -> Result<()> {
    if port == 0 {
        return Err(new_error(CODE_LISTEN_ADDR, "port cannot be 0".to_string()));
    }
    Ok(())
}

/// 校验监听地址：`:port`、`ip:port`、`[v6]:port`，以及（不推荐的）`hostname:port`。
///
/// # ⚠️ 禁止用于请求参数
/// 仅主机名形式会走 [`ToSocketAddrs`] 的**同步阻塞 DNS 解析**。本函数只应校验
/// 启动期的监听地址配置。若把它挂到请求入参上（`#[validate(custom(...))]`），
/// 攻击者可以用一个解析极慢的域名把 tokio worker 线程堵死。
///
/// IP 字面量（含 IPv6）走快路径直接解析，**不触发 DNS**——实际部署几乎都是这一类，
/// 阻塞路径只在配置里真写了主机名时才会走到。
pub fn x_listen_addr(addr: &str) -> Result<()> {
    if is_disabled(CODE_LISTEN_ADDR) {
        return Ok(());
    }
    // `:port` 简写（监听所有网卡）
    if let Some(value) = addr.strip_prefix(':') {
        let port = value.parse::<u16>().map_err(|_| {
            new_error(
                CODE_LISTEN_ADDR,
                "port must be a number between 1 and 65535".to_string(),
            )
        })?;
        return check_listen_port(port);
    }

    // 快路径：IP 字面量（`127.0.0.1:8080` / `[::1]:8080`），不做 DNS
    if let Ok(socket_addr) = addr.parse::<SocketAddr>() {
        return check_listen_port(socket_addr.port());
    }

    // 回退：主机名形式，需要阻塞 DNS —— 见上方警告
    let mut addrs = addr
        .to_socket_addrs()
        .map_err(|_| new_error(CODE_LISTEN_ADDR, "invalid address format".to_string()))?;
    let Some(first) = addrs.next() else {
        return Err(new_error(
            CODE_LISTEN_ADDR,
            "no valid address found".to_string(),
        ));
    };
    check_listen_port(first.port())
}

/// 校验 UUID：必须是 36 字符的标准带连字符形式（`8-4-4-4-12`，全十六进制）。
///
/// 此前只比对长度，36 个 `a` 也能通过——而本校验器用在 token / 验证码 / 资源 ID
/// 等入参上，等于没有防护。现走 `uuid` crate 的严格解析。
pub fn x_uuid(value: &str) -> Result<()> {
    if is_disabled(CODE_UUID) {
        return Ok(());
    }
    // 仍然限定 36 字符：`Uuid::try_parse` 另外还接受无连字符(32)、花括号(38)、
    // URN(45) 等形式，只认标准写法可避免同一 ID 出现多种等价文本。
    if value.len() != 36 || Uuid::try_parse(value).is_err() {
        return Err(new_error(CODE_UUID, "invalid uuid format".to_string()));
    }
    Ok(())
}

/// 校验 SHA-256 十六进制摘要：必须是 64 个十六进制字符。
///
/// 此前只比对长度，64 个 `z` 也能通过。大小写均接受——本函数只管格式，
/// 大小写归一化属调用方的职责。
pub fn x_sha256(value: &str) -> Result<()> {
    if is_disabled(CODE_SHA256) {
        return Ok(());
    }
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(new_error(CODE_SHA256, "invalid sha256 format".to_string()));
    }
    Ok(())
}

pub fn x_file_name(name: &str) -> Result<()> {
    if is_disabled(CODE_FILE_NAME) {
        return Ok(());
    }
    if name.is_empty() {
        return Err(new_error(
            CODE_FILE_NAME,
            "file name cannot be empty".to_string(),
        ));
    }
    if Path::new(name).extension().is_none() {
        return Err(new_error(
            CODE_FILE_NAME,
            "file name must have an extension".to_string(),
        ));
    }
    Ok(())
}

pub fn x_file_group(group: &str) -> Result<()> {
    if is_disabled(CODE_FILE_GROUP) {
        return Ok(());
    }
    validate_ascii_name(group, CODE_FILE_GROUP, 100, "file group")
}

pub fn x_image_format(format: &str) -> Result<()> {
    if is_disabled(CODE_IMAGE_FORMAT) {
        return Ok(());
    }
    if !["avif", "webp", "png", "jpeg"].contains(&format) {
        return Err(new_error(
            CODE_IMAGE_FORMAT,
            "invalid image format".to_string(),
        ));
    }
    Ok(())
}

pub fn x_image_quality(quality: u8) -> Result<()> {
    if is_disabled(CODE_IMAGE_QUALITY) {
        return Ok(());
    }
    if !(50..=100).contains(&quality) {
        return Err(new_error(
            CODE_IMAGE_QUALITY,
            "image quality must be between 50 and 100".to_string(),
        ));
    }
    Ok(())
}

pub fn x_schema_name(name: &str) -> Result<()> {
    if is_disabled(CODE_SCHEMA_NAME) {
        return Ok(());
    }
    validate_ascii_name(name, CODE_SCHEMA_NAME, 50, "schema name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_requires_real_format_not_just_length() {
        assert!(x_uuid("67e55044-10b1-426f-9247-bb680e5fe0c8").is_ok());
        // 大写十六进制同样是合法 UUID
        assert!(x_uuid("67E55044-10B1-426F-9247-BB680E5FE0C8").is_ok());

        // 回归守卫：36 个 `a` 长度对但不是 UUID，旧实现会放行
        assert!(x_uuid(&"a".repeat(36)).is_err());
        // 连字符位置错误
        assert!(x_uuid("67e550441-0b1-426f-9247-bb680e5fe0c8").is_err());
        // 含非十六进制字符
        assert!(x_uuid("67e55044-10b1-426f-9247-bb680e5fe0cZ").is_err());
        // 长度不符的其它合法 UUID 写法一律不接受，避免同一 ID 多种文本
        assert!(x_uuid("67e5504410b1426f9247bb680e5fe0c8").is_err());
        assert!(x_uuid("{67e55044-10b1-426f-9247-bb680e5fe0c8}").is_err());
        assert!(x_uuid("").is_err());
    }

    #[test]
    fn sha256_requires_hex_not_just_length() {
        assert!(x_sha256(&"a".repeat(64)).is_ok());
        assert!(x_sha256(&"F".repeat(64)).is_ok());
        assert!(
            x_sha256("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824").is_ok()
        );

        // 回归守卫：64 个 `z` 长度对但不是十六进制，旧实现会放行
        assert!(x_sha256(&"z".repeat(64)).is_err());
        assert!(x_sha256(&"a".repeat(63)).is_err());
        assert!(x_sha256(&"a".repeat(65)).is_err());
        assert!(x_sha256("").is_err());
    }

    /// 只覆盖不触发 DNS 的路径，测试保持离线可跑。
    #[test]
    fn listen_addr_accepts_ip_literals_without_dns() {
        assert!(x_listen_addr(":8080").is_ok());
        assert!(x_listen_addr("127.0.0.1:8080").is_ok());
        assert!(x_listen_addr("0.0.0.0:3000").is_ok());
        // IPv6 字面量此前也能过（走 to_socket_addrs），现在走快路径不做 DNS
        assert!(x_listen_addr("[::1]:8080").is_ok());
        assert!(x_listen_addr("[::]:8080").is_ok());
    }

    #[test]
    fn listen_addr_rejects_port_zero_consistently() {
        // 三种写法都必须拒绝——此前只有 `:0` 被拒，`0.0.0.0:0` 会放行
        assert!(x_listen_addr(":0").is_err());
        assert!(x_listen_addr("0.0.0.0:0").is_err());
        assert!(x_listen_addr("[::1]:0").is_err());
    }

    #[test]
    fn listen_addr_rejects_malformed_port() {
        assert!(x_listen_addr(":not-a-number").is_err());
        assert!(x_listen_addr(":70000").is_err());
    }
}
