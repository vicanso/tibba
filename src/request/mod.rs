use once_cell::sync::OnceCell;
use std::time::Duration;

mod instance;

pub fn get_baidu_instance() -> &'static Instance<instance::CommonErrorHandler> {
    static BAIDU: OnceCell<Instance<instance::CommonErrorHandler>> = OnceCell::new();
    BAIDU.get_or_init(|| {
        Instance::new(
            "baidu",
            "https://baidu.com/",
            Duration::from_secs(60),
            instance::CommonErrorHandler {},
        )
    })
}

pub use instance::Instance;
