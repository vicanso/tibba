use axum::http::Extensions;

tokio::task_local! {
    pub static TRACE_ID: String;
    pub static ACCOUNT: String;
    pub static DEVICE_ID: String;
}

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
