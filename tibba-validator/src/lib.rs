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

use std::env;
use validator::ValidationError;

type Result<T> = std::result::Result<T, ValidationError>;

mod common;
mod user;

fn is_disabled(code: &str) -> bool {
    let key = code.replace("-", "_").to_lowercase();
    env::var(&key).unwrap_or_default() == "*"
}
fn new_error(code: &'static str, message: String) -> ValidationError {
    ValidationError::new(code).with_message(message.into())
}

fn validate_ascii_name(
    name: &str,
    code: &'static str,
    max_len: usize,
    field_name: &str,
) -> Result<()> {
    if name.is_empty() {
        // 修复：直接传递 String，而不是它的引用
        return Err(new_error(code, format!("{field_name} cannot be empty")));
    }
    if !name.is_ascii() {
        return Err(new_error(code, format!("{field_name} must be ASCII")));
    }
    if name.len() > max_len {
        return Err(new_error(
            code,
            format!("{field_name} must be less than {max_len} characters"),
        ));
    }
    Ok(())
}

// user validate
pub const CODE_USER_ACCOUNT: &str = "x-user-account";
pub const CODE_USER_PASSWORD: &str = "x-user-password";
pub const CODE_USER_EMAIL: &str = "x-user-email";
pub const CODE_USER_ROLES: &str = "x-user-roles";
pub const CODE_USER_GROUPS: &str = "x-user-groups";

// common validate
pub const CODE_LISTEN_ADDR: &str = "x-listen-addr";
pub const CODE_UUID: &str = "x-uuid";
pub const CODE_SHA256: &str = "x-sha256";
pub const CODE_FILE_NAME: &str = "x-file-name";
pub const CODE_FILE_GROUP: &str = "x-file-group";
pub const CODE_SCHEMA_NAME: &str = "x-schema-name";
pub const CODE_IMAGE_FORMAT: &str = "x-image-format";
pub const CODE_IMAGE_QUALITY: &str = "x-image-quality";

pub use common::*;
pub use user::*;
