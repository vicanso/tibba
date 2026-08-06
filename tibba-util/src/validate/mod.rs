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

//! `validator` crate 的自定义校验函数（`x_*`）与错误码常量（`CODE_*`）。
//!
//! 每个校验器可通过同名环境变量（`-` 换 `_`、转小写、值为 `*`）临时关闭，
//! 便于本地开发绕过格式限制，见 [`is_disabled`]。
//!
//! 模块名用 `validate` 而非 `validator`，以免与外部 `validator` crate 撞名。

use std::collections::HashSet;
use std::env;
use std::sync::LazyLock;
use validator::ValidationError;

type Result<T> = std::result::Result<T, ValidationError>;

mod common;
mod user;

/// 校验器码对应的环境变量名：`-` 换 `_` 并转小写。
/// 例：`x-user-account` → `x_user_account`。
fn env_key(code: &str) -> String {
    code.replace('-', "_").to_lowercase()
}

/// 按给定的环境查询函数，挑出被关闭（值为 `*`）的校验器码。
///
/// `lookup` 抽成参数是为了单测能注入假环境——`std::env::set_var` 在多线程
/// test binary 中与其它线程读环境存在竞态（Rust 2024 已标记为 `unsafe`），
/// 与 `tibba-config` 的 `build_with_env` same trick。
fn collect_disabled(lookup: impl Fn(&str) -> Option<String>) -> HashSet<&'static str> {
    ALL_CODES
        .iter()
        .copied()
        .filter(|code| lookup(&env_key(code)).as_deref() == Some("*"))
        .collect()
}

/// 首次校验时快照一次「已关闭的校验器」集合。
///
/// 此前每次调用都要 `code.replace("-","_").to_lowercase()`（两次堆分配）再
/// `env::var` 遍历一遍环境，而这是**每请求 × 每个被校验字段**都会走的路径——
/// 单个登录请求就有 3-4 个字段。
///
/// 快照而非每次读取是有意为之：这些开关只用于本地开发，进程启动前设好即可。
/// Rust 2024 起 `std::env::set_var` 已是 `unsafe`（多线程下与读环境竞态），
/// 本就不该在运行中改。
static DISABLED_CODES: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| collect_disabled(|key| env::var(key).ok()));

/// 该校验器是否已被环境变量关闭。O(1) 查表，零分配。
fn is_disabled(code: &str) -> bool {
    DISABLED_CODES.contains(code)
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

/// 声明校验器错误码常量，并同步生成 [`ALL_CODES`] 注册表。
///
/// 用宏而不是「一堆常量 + 手写一份列表」：漏登记不会有任何编译错误，只会让该
/// 校验器的环境变量开关静默失效——这种 bug 极难被发现。宏让两者不可能脱节。
macro_rules! validate_codes {
    ($($(#[$attr:meta])* $name:ident = $value:literal;)+) => {
        $($(#[$attr])* pub const $name: &str = $value;)+

        /// 全部校验器码，由 `validate_codes!` 统一生成，不会漏项。
        const ALL_CODES: &[&str] = &[$($name),+];
    };
}

validate_codes! {
    // user validate
    CODE_USER_ACCOUNT = "x-user-account";
    CODE_USER_PASSWORD = "x-user-password";
    CODE_USER_EMAIL = "x-user-email";
    CODE_USER_ROLES = "x-user-roles";
    CODE_USER_GROUPS = "x-user-groups";

    // common validate
    CODE_LISTEN_ADDR = "x-listen-addr";
    CODE_UUID = "x-uuid";
    CODE_SHA256 = "x-sha256";
    CODE_FILE_NAME = "x-file-name";
    CODE_FILE_GROUP = "x-file-group";
    CODE_SCHEMA_NAME = "x-schema-name";
    CODE_IMAGE_FORMAT = "x-image-format";
    CODE_IMAGE_QUALITY = "x-image-quality";
}

pub use common::*;
pub use user::*;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    #[test]
    fn env_key_maps_code_to_variable_name() {
        assert_eq!(env_key("x-user-account"), "x_user_account");
        assert_eq!(env_key("x-uuid"), "x_uuid");
        assert_eq!(env_key("X-Image-Quality"), "x_image_quality");
    }

    /// 注册表由宏生成，这里守住两个不变量：无重复、命名统一。
    #[test]
    fn all_codes_registry_is_consistent() {
        let unique: HashSet<&&str> = ALL_CODES.iter().collect();
        assert_eq!(unique.len(), ALL_CODES.len(), "ALL_CODES 不得有重复项");
        for code in ALL_CODES {
            assert!(code.starts_with("x-"), "{code} 应以 x- 开头");
            assert_eq!(*code, code.to_lowercase(), "{code} 应全小写");
        }
    }

    /// 只有值恰为 `*` 才算关闭；其它值（含空串）一律视为开启。
    #[test]
    fn only_star_disables_a_validator() {
        let env: HashMap<String, String> = [
            ("x_user_account", "*"),
            ("x_uuid", "1"),      // 非 `*`，不算关闭
            ("x_sha256", ""),     // 空串，不算关闭
            ("x_unrelated", "*"), // 不在注册表里，忽略
        ]
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();

        let disabled = collect_disabled(|key| env.get(key).cloned());
        assert_eq!(disabled, HashSet::from([CODE_USER_ACCOUNT]));
    }

    #[test]
    fn empty_environment_disables_nothing() {
        assert!(collect_disabled(|_| None).is_empty());
    }

    /// 注册表里的每一项都必须能被自己的环境变量关闭。
    /// 若将来有人绕过宏手写常量而漏登记，本例会失败。
    #[test]
    fn every_registered_code_is_switchable() {
        for code in ALL_CODES {
            let key = env_key(code);
            let disabled = collect_disabled(|k| (k == key).then(|| "*".to_string()));
            assert_eq!(
                disabled,
                HashSet::from([*code]),
                "{code} 的开关未生效（可能未登记进 ALL_CODES）"
            );
        }
    }
}
