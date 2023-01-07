tokio::task_local! {
    pub static TRACE_ID: String;
    pub static ACCOUNT: String;
}

pub fn clone_value_from_context<T>(value: &T) -> T
where
    T: Clone,
{
    value.clone()
}
