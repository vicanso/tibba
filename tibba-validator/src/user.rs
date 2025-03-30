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
    CODE_USER_ACCOUNT, CODE_USER_EMAIL, CODE_USER_GROUPS, CODE_USER_PASSWORD, CODE_USER_ROLES,
};
use validator::{ValidateEmail, ValidationError};

type Result<T> = std::result::Result<T, ValidationError>;

pub fn x_user_account(user: &str) -> Result<()> {
    if is_disabled(CODE_USER_ACCOUNT) {
        return Ok(());
    }
    if !user.is_ascii() {
        return Err(ValidationError::new(CODE_USER_ACCOUNT));
    }
    if user.len() < 2 || user.len() > 20 {
        return Err(ValidationError::new(CODE_USER_ACCOUNT));
    }
    Ok(())
}

pub fn x_user_password(password: &str) -> Result<()> {
    if is_disabled(CODE_USER_PASSWORD) {
        return Ok(());
    }
    if !password.is_ascii() {
        return Err(ValidationError::new(CODE_USER_PASSWORD));
    }
    if password.len() < 32 {
        return Err(ValidationError::new(CODE_USER_PASSWORD));
    }
    Ok(())
}

pub fn x_user_email(email: &str) -> Result<()> {
    if is_disabled(CODE_USER_EMAIL) {
        return Ok(());
    }
    if !email.validate_email() {
        return Err(ValidationError::new(CODE_USER_EMAIL));
    }
    Ok(())
}

pub fn x_user_roles(roles: &[String]) -> Result<()> {
    if is_disabled(CODE_USER_ROLES) {
        return Ok(());
    }
    for role in roles {
        if !role.is_ascii() {
            return Err(ValidationError::new(CODE_USER_ROLES));
        }
    }
    Ok(())
}

pub fn x_user_groups(groups: &[String]) -> Result<()> {
    if is_disabled(CODE_USER_GROUPS) {
        return Ok(());
    }
    for group in groups {
        if !group.is_ascii() {
            return Err(ValidationError::new(CODE_USER_GROUPS));
        }
    }
    Ok(())
}
