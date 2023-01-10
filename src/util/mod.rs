mod context;
mod duration;
mod http;
mod string;

pub use self::http::{
    insert_header, read_http_body, set_header_if_not_exist, set_no_cache_if_not_exist,
};
pub use context::{
    clone_value_from_context, generate_device_id_cookie, get_account_from_context,
    get_device_id_from_cookie, set_account_to_context, Account, ACCOUNT, DEVICE_ID, TRACE_ID,
};
pub use duration::duration_to_string;
pub use string::random_string;
