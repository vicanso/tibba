mod compress;
mod context;
mod duration;
mod http;
mod string;

pub use self::http::{
    get_header_value, insert_header, read_http_body, set_header_if_not_exist,
    set_no_cache_if_not_exist,
};
pub use compress::{snappy_decode, snappy_encode, zstd_decode, zstd_encode};
pub use context::{
    clone_value_from_context, generate_device_id_cookie, get_account_from_context,
    get_device_id_from_cookie, set_account_to_context, Account, ACCOUNT, DEVICE_ID, STARTED_AT,
    TRACE_ID,
};
pub use duration::duration_to_string;
pub use string::{json_get, random_string};
