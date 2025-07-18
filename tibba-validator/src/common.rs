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

use super::is_disabled;
use super::{
    CODE_FILE_GROUP, CODE_FILE_NAME, CODE_IMAGE_FORMAT, CODE_IMAGE_QUALITY, CODE_LISTEN_ADDR,
    CODE_SCHEMA_NAME, CODE_SHA256, CODE_UUID,
};
use std::borrow::Cow;
use std::net::ToSocketAddrs;
use std::path::Path;
use validator::ValidationError;

type Result<T> = std::result::Result<T, ValidationError>;

pub fn x_listen_addr(addr: &str) -> Result<()> {
    if is_disabled(CODE_LISTEN_ADDR) {
        return Ok(());
    }
    // validate port
    if let Some(value) = addr.strip_prefix(':') {
        let port = value.parse::<u16>().map_err(|_| {
            ValidationError::new(CODE_LISTEN_ADDR)
                .with_message(Cow::from("Port must be a number between 1 and 65535"))
        })?;
        if port == 0 {
            return Err(
                ValidationError::new(CODE_LISTEN_ADDR).with_message(Cow::from("Port cannot be 0"))
            );
        }
        return Ok(());
    }

    // validate address to socket addrs
    let addr_result = addr.to_socket_addrs();
    match addr_result {
        Ok(mut addrs) => {
            // ensure at least one valid address
            if addrs.next().is_none() {
                return Err(ValidationError::new(CODE_LISTEN_ADDR)
                    .with_message(Cow::from("No valid address found")));
            }
        }
        Err(_) => {
            return Err(ValidationError::new(CODE_LISTEN_ADDR)
                .with_message(Cow::from("Invalid address format")));
        }
    }
    Ok(())
}

pub fn x_uuid(uuid: &str) -> Result<()> {
    if is_disabled(CODE_UUID) {
        return Ok(());
    }
    if uuid.len() != 36 {
        return Err(ValidationError::new(CODE_UUID).with_message(Cow::from("Invalid uuid format")));
    }
    Ok(())
}

pub fn x_sha256(sha256: &str) -> Result<()> {
    if is_disabled(CODE_SHA256) {
        return Ok(());
    }
    if sha256.len() != 64 {
        return Err(
            ValidationError::new(CODE_SHA256).with_message(Cow::from("Invalid sha256 format"))
        );
    }
    Ok(())
}

pub fn x_file_name(name: &str) -> Result<()> {
    if is_disabled(CODE_FILE_NAME) {
        return Ok(());
    }
    if name.is_empty() {
        return Err(ValidationError::new(CODE_FILE_NAME)
            .with_message(Cow::from("File name cannot be empty")));
    }
    if Path::new(name).extension().is_none() {
        return Err(ValidationError::new(CODE_FILE_NAME)
            .with_message(Cow::from("File name must have an extension")));
    }
    Ok(())
}

pub fn x_file_group(group: &str) -> Result<()> {
    if is_disabled(CODE_FILE_GROUP) {
        return Ok(());
    }
    if group.is_empty() {
        return Err(ValidationError::new(CODE_FILE_GROUP)
            .with_message(Cow::from("File group cannot be empty")));
    }
    if !group.is_ascii() {
        return Err(ValidationError::new(CODE_FILE_GROUP)
            .with_message(Cow::from("File group must be ASCII")));
    }
    if group.len() > 100 {
        return Err(ValidationError::new(CODE_FILE_GROUP)
            .with_message(Cow::from("File group must be less than 100 characters")));
    }
    Ok(())
}

pub fn x_image_format(format: &str) -> Result<()> {
    if is_disabled(CODE_IMAGE_FORMAT) {
        return Ok(());
    }
    if !["avif", "webp", "png", "jpeg"].contains(&format) {
        return Err(
            ValidationError::new(CODE_IMAGE_FORMAT).with_message(Cow::from("Invalid image format"))
        );
    }
    Ok(())
}

pub fn x_image_quality(quality: u8) -> Result<()> {
    if is_disabled(CODE_IMAGE_QUALITY) {
        return Ok(());
    }
    if !(50..=100).contains(&quality) {
        return Err(ValidationError::new(CODE_IMAGE_QUALITY)
            .with_message(Cow::from("Image quality must be between 50 and 100")));
    }
    Ok(())
}

pub fn x_schema_name(name: &str) -> Result<()> {
    if is_disabled(CODE_SCHEMA_NAME) {
        return Ok(());
    }
    if name.is_empty() {
        return Err(ValidationError::new(CODE_SCHEMA_NAME)
            .with_message(Cow::from("Schema name cannot be empty")));
    }
    if !name.is_ascii() {
        return Err(ValidationError::new(CODE_SCHEMA_NAME)
            .with_message(Cow::from("Schema name must be ASCII")));
    }
    if name.len() > 50 {
        return Err(ValidationError::new(CODE_SCHEMA_NAME)
            .with_message(Cow::from("Schema name must be less than 50 characters")));
    }
    Ok(())
}
