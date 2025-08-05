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

use std::borrow::Cow;
use std::env;
use validator::ValidationError;

mod common;
mod user;

fn is_disabled(code: &str) -> bool {
    let key = code.replace("-", "_").to_lowercase();
    env::var(&key).unwrap_or_default() == "*"
}
fn new_error(code: &'static str, message: &'static str) -> ValidationError {
    ValidationError::new(code).with_message(Cow::from(message))
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
