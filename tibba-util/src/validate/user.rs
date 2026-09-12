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

use super::{has_control_or_space, is_disabled, is_identifier_char, new_error};
use super::{
    CODE_USER_ACCOUNT, CODE_USER_ACCOUNT_STRICT, CODE_USER_EMAIL, CODE_USER_GROUPS,
    CODE_USER_PASSWORD, CODE_USER_ROLES,
};
use validator::{ValidateEmail, ValidationError};

type Result<T> = std::result::Result<T, ValidationError>;

/// 账号长度区间（字节，已限定 ASCII 故等同字符数）。
const ACCOUNT_MIN_LEN: usize = 2;
const ACCOUNT_MAX_LEN: usize = 20;

/// 口令长度区间。下限 32 是因为本项目传的是客户端 `sha256(明文)`（64 字符）；
/// 上限存在的意义是别让慢哈希的输入无上界——`tibba-crypto` 那层还有一道
/// 1 KiB 的闸（`DEFAULT_MAX_SECRET_LEN`），这里先在参数层挡掉，省得把明显
/// 不合规的输入一路带到 Argon2 前面才拒绝。
const PASSWORD_MIN_LEN: usize = 32;
const PASSWORD_MAX_LEN: usize = 256;

/// 单个角色 / 用户组名的长度上限。
const ROLE_MAX_LEN: usize = 64;
/// 一次可提交的角色 / 用户组数量上限。
const ROLE_MAX_COUNT: usize = 64;

/// 账号格式校验（登录 / 找回密码等**存量**账号会走到的路径）。
///
/// 只做「任何账号都不该违反」的底线检查：ASCII、长度 2–20、无控制字符与空格。
///
/// # 为什么这里不上字符白名单
/// 本函数同时挂在登录与找回密码的入参上。历史上它只校验 ASCII + 长度，库里
/// 完全可能存在含 `@` 等字符的账号（例如早期直接拿邮箱注册）。在登录路径上
/// 收紧白名单等于把这些人挡在门外——那是一次数据迁移，不是一次校验加固。
///
/// 新账号走 [`x_user_account_strict`]，白名单在**注册**处收口；存量账号则在
/// 这里止步于「不含能伪造日志 / 截断字符串的字符」这条底线。
pub fn x_user_account(user: &str) -> Result<()> {
    if is_disabled(CODE_USER_ACCOUNT) {
        return Ok(());
    }
    if !user.is_ascii() {
        return Err(new_error(
            CODE_USER_ACCOUNT,
            "account must be ASCII".to_string(),
        ));
    }
    if user.len() < ACCOUNT_MIN_LEN || user.len() > ACCOUNT_MAX_LEN {
        return Err(new_error(
            CODE_USER_ACCOUNT,
            format!("account must be {ACCOUNT_MIN_LEN}-{ACCOUNT_MAX_LEN} characters"),
        ));
    }
    // 控制字符 / 空格：账号会被写进日志、审计记录与会话，放行等于开放日志伪造
    if has_control_or_space(user) {
        return Err(new_error(
            CODE_USER_ACCOUNT,
            "account must not contain control characters or spaces".to_string(),
        ));
    }
    Ok(())
}

/// **新建**账号的格式校验：在 [`x_user_account`] 之上追加字符白名单。
///
/// 允许 `[A-Za-z0-9_.-]`，且首字符必须是字母或数字（避免 `-x` 这类在命令行 /
/// 各类 CLI 参数里会被当成选项的形态）。
///
/// 与 OAuth 侧自动分配账号名的清洗规则一致（见 `tibba-router-user` 的
/// `allocate_account`，它本就只保留 `[A-Za-z0-9_-]`），因此收紧白名单不会让
/// 第三方登录注册不进来。
pub fn x_user_account_strict(user: &str) -> Result<()> {
    if is_disabled(CODE_USER_ACCOUNT_STRICT) {
        return Ok(());
    }
    // 先过底线检查，长度 / ASCII 的报错信息复用同一套
    x_user_account(user)?;

    if !user.chars().all(is_identifier_char) {
        return Err(new_error(
            CODE_USER_ACCOUNT_STRICT,
            "account may only contain letters, digits, '_', '-' and '.'".to_string(),
        ));
    }
    if !user.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return Err(new_error(
            CODE_USER_ACCOUNT_STRICT,
            "account must start with a letter or digit".to_string(),
        ));
    }
    Ok(())
}

/// 口令格式校验：ASCII，长度 [`PASSWORD_MIN_LEN`]–[`PASSWORD_MAX_LEN`]。
pub fn x_user_password(password: &str) -> Result<()> {
    if is_disabled(CODE_USER_PASSWORD) {
        return Ok(());
    }
    if !password.is_ascii() {
        return Err(new_error(
            CODE_USER_PASSWORD,
            "password must be ASCII".to_string(),
        ));
    }
    if password.len() < PASSWORD_MIN_LEN {
        return Err(new_error(
            CODE_USER_PASSWORD,
            format!("password must be at least {PASSWORD_MIN_LEN} characters"),
        ));
    }
    // 上限：慢哈希的输入不能无界，见 PASSWORD_MAX_LEN
    if password.len() > PASSWORD_MAX_LEN {
        return Err(new_error(
            CODE_USER_PASSWORD,
            format!("password must be at most {PASSWORD_MAX_LEN} characters"),
        ));
    }
    Ok(())
}

pub fn x_user_email(email: &str) -> Result<()> {
    if is_disabled(CODE_USER_EMAIL) {
        return Ok(());
    }
    if !email.validate_email() {
        return Err(new_error(
            CODE_USER_EMAIL,
            "invalid email format".to_string(),
        ));
    }
    Ok(())
}

/// 校验一组标识符（角色 / 用户组）：数量、单项长度与字符集。
///
/// 三条限制缺一不可：只限字符集时，一次请求可以塞进十万个角色把 RBAC 判定
/// 拖垮；只限数量时，单个角色名可以长到把日志行撑爆。
fn validate_identifiers(values: &[String], code: &'static str, field: &str) -> Result<()> {
    if values.len() > ROLE_MAX_COUNT {
        return Err(new_error(
            code,
            format!("at most {ROLE_MAX_COUNT} {field} are allowed"),
        ));
    }
    for value in values {
        if value.is_empty() {
            return Err(new_error(code, format!("{field} must not be empty")));
        }
        if value.len() > ROLE_MAX_LEN {
            return Err(new_error(
                code,
                format!("each of {field} must be at most {ROLE_MAX_LEN} characters"),
            ));
        }
        if !value.chars().all(is_identifier_char) {
            return Err(new_error(
                code,
                format!("{field} may only contain letters, digits, '_', '-' and '.'"),
            ));
        }
    }
    Ok(())
}

pub fn x_user_roles(roles: &[String]) -> Result<()> {
    if is_disabled(CODE_USER_ROLES) {
        return Ok(());
    }
    validate_identifiers(roles, CODE_USER_ROLES, "roles")
}

pub fn x_user_groups(groups: &[String]) -> Result<()> {
    if is_disabled(CODE_USER_GROUPS) {
        return Ok(());
    }
    validate_identifiers(groups, CODE_USER_GROUPS, "groups")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_enforces_ascii_and_length() {
        assert!(x_user_account("ab").is_ok());
        assert!(x_user_account(&"a".repeat(ACCOUNT_MAX_LEN)).is_ok());

        assert!(x_user_account("a").is_err());
        assert!(x_user_account(&"a".repeat(ACCOUNT_MAX_LEN + 1)).is_err());
        assert!(x_user_account("用户名").is_err());
    }

    /// **回归守卫**：控制字符与空格必须挡住。
    ///
    /// 旧实现只查 ASCII + 长度，`"ab\ncd"` 能通过——账号会原样进日志，
    /// 于是任何人都可以在审计日志里伪造出一整行。
    #[test]
    fn account_rejects_control_characters_and_spaces() {
        for bad in ["ab\ncd", "ab\rcd", "ab\tcd", "ab cd", "ab\0cd"] {
            assert!(
                x_user_account(bad).is_err(),
                "{bad:?} 含控制字符/空格，应被拒绝"
            );
        }
    }

    /// 存量账号里可能出现的字符（如邮箱形态）在登录路径上仍需放行，
    /// 收紧只发生在注册路径。
    #[test]
    fn account_keeps_legacy_shapes_loginable() {
        assert!(x_user_account("a@b.com").is_ok());
        assert!(x_user_account("user+tag").is_ok());
    }

    #[test]
    fn strict_account_applies_allowlist() {
        for good in ["ab", "user_1", "gh_octocat", "a.b-c", "9lives"] {
            assert!(x_user_account_strict(good).is_ok(), "{good} 应当通过");
        }
        for bad in ["a@b.com", "user+tag", "_leading", "-leading", ".dot", "a/b"] {
            assert!(x_user_account_strict(bad).is_err(), "{bad} 应当被拒绝");
        }
        // 底线检查同样生效
        assert!(x_user_account_strict("a").is_err());
        assert!(x_user_account_strict("ab cd").is_err());
    }

    #[test]
    fn password_is_bounded_on_both_ends() {
        let sha256_hex = "a".repeat(64);
        assert!(x_user_password(&sha256_hex).is_ok());

        assert!(x_user_password(&"a".repeat(PASSWORD_MIN_LEN - 1)).is_err());
        // 回归守卫：此前没有上限，一个 1MB 的「口令」会一路走到 Argon2 前面
        assert!(x_user_password(&"a".repeat(PASSWORD_MAX_LEN + 1)).is_err());
        assert!(x_user_password(&"密".repeat(64)).is_err());
    }

    #[test]
    fn roles_and_groups_are_bounded() {
        assert!(x_user_roles(&["admin".to_string(), "su".to_string()]).is_ok());
        assert!(x_user_groups(&["team-a".to_string()]).is_ok());

        // 回归守卫：此前只查 ASCII，下面三类全部会被放行
        assert!(x_user_roles(&["a".repeat(ROLE_MAX_LEN + 1)]).is_err());
        assert!(x_user_roles(&vec!["r".to_string(); ROLE_MAX_COUNT + 1]).is_err());
        assert!(x_user_roles(&["admin\nsu".to_string()]).is_err());
        assert!(x_user_roles(&[String::new()]).is_err());
        assert!(x_user_groups(&["team:a".to_string()]).is_err());
    }

    #[test]
    fn email_validation_reports_a_message() {
        let err = x_user_email("not-an-email").unwrap_err();
        assert!(err.message.is_some(), "校验错误应带上可读信息");
        assert!(x_user_email("a@b.com").is_ok());
    }
}
