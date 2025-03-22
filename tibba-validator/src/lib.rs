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

mod common;
mod user;

fn is_disabled(code: &str) -> bool {
    let key = code.replace("-", "_").to_lowercase();
    if env::var(&key).unwrap_or_default() == "*" {
        return true;
    }
    false
}

pub const CODE_USER_ACCOUNT: &str = "x-user-account";
pub const CODE_USER_PASSWORD: &str = "x-user-password";
pub const CODE_LISTEN_ADDR: &str = "x-listen-addr";
pub const CODE_UUID: &str = "x-uuid";
pub const CODE_FILE_NAME: &str = "x-file-name";

pub use common::*;
pub use user::*;
