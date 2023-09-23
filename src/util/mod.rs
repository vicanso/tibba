mod compress;
mod context;
mod datetime;
mod duration;
mod http;
mod number;
mod string;

use crate::config::get_env;

pub use self::http::{
    get_header_value, insert_header, read_http_body, set_header_if_not_exist,
    set_no_cache_if_not_exist,
};
pub use compress::Error as CompressError;
pub use compress::{lz4_decode, lz4_encode, zstd_decode, zstd_encode};
pub use context::{
    generate_device_id_cookie, get_account_from_context, get_device_id_from_cookie,
    set_account_to_context, Account,
};
pub use datetime::{from_timestamp, now};
pub use duration::duration_to_string;
pub use number::float_to_fixed;
pub use string::{json_get, random_string};

/// 是否开发环境
/// 用于针对本地开发时的判断
pub fn is_development() -> bool {
    get_env() == "dev"
}
/// 是否测试环境
pub fn is_test() -> bool {
    get_env() == "test"
}

/// 是否生产环境
pub fn is_production() -> bool {
    get_env() == "production"
}
