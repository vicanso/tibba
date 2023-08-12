use once_cell::sync::OnceCell;
use std::time::Duration;

mod instance;

pub fn get_charts_instance() -> &'static Instance<instance::CommonErrorHandler> {
    static CHARTS: OnceCell<Instance<instance::CommonErrorHandler>> = OnceCell::new();
    CHARTS.get_or_init(|| {
        Instance::new(
            "baidu",
            "https://charts.npmtrend.com/api",
            Duration::from_secs(60),
            instance::CommonErrorHandler {},
        )
    })
}

pub fn get_image_optim_instance() -> &'static Instance<instance::CommonErrorHandler> {
    static OPTIM: OnceCell<Instance<instance::CommonErrorHandler>> = OnceCell::new();
    OPTIM.get_or_init(|| {
        Instance::new(
            "image-optim",
            "http://rasp:6011",
            Duration::from_secs(60),
            instance::CommonErrorHandler {},
        )
    })
}

pub use instance::Instance;
