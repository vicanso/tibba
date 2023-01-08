use axum::http::Extensions;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use cookie::time::{Duration, OffsetDateTime};

use super::random_string;

tokio::task_local! {
    pub static TRACE_ID: String;
    pub static ACCOUNT: String;
    pub static DEVICE_ID: String;
}

static DEVICE_ID_NAME: &str = "device";

pub struct Account(String);

impl Account {
    pub fn new(account: String) -> Account {
        Account(account)
    }
}

pub fn clone_value_from_context<T>(value: &T) -> T
where
    T: Clone,
{
    value.clone()
}

pub fn set_account_to_context(exts: &mut Extensions, account: Account) -> Option<Account> {
    exts.insert(account)
}

pub fn get_account_from_context(exts: &Extensions) -> String {
    if let Some(account) = exts.get::<Account>() {
        return account.0.clone();
    }
    "".to_string()
}

pub fn get_device_id_from_cookie(jar: &CookieJar) -> String {
    if let Some(value) = jar.get(DEVICE_ID_NAME) {
        return value.value().to_string();
    }
    "".to_string()
}

pub fn generate_device_id_cookie() -> Cookie<'static> {
    let mut now = OffsetDateTime::now_utc();
    now += Duration::weeks(52);
    Cookie::build(DEVICE_ID_NAME, random_string(10))
        .http_only(true)
        .expires(now)
        .path("/")
        .finish()
}
