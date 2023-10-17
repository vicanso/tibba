use once_cell::sync::OnceCell;
use std::time::Duration;

mod instance;
use instance::CommonInterceptor;

pub fn must_get_httpbin_instance() -> &'static Instance<CommonInterceptor> {
    static CHARTS: OnceCell<Instance<CommonInterceptor>> = OnceCell::new();
    CHARTS
        .get_or_try_init(|| {
            let service = "httpbin";
            Instance::new(
                service,
                "https://httpbin.org",
                Duration::from_secs(60),
                CommonInterceptor::new(service),
            )
        })
        .unwrap()
}

pub use instance::Instance;
