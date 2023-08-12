use once_cell::sync::OnceCell;
use std::time::Duration;

mod instance;
use instance::CommonInterceptor;

pub fn get_charts_instance() -> &'static Instance<CommonInterceptor> {
    static CHARTS: OnceCell<Instance<CommonInterceptor>> = OnceCell::new();
    CHARTS.get_or_init(|| {
        let service = "charts";
        Instance::new(
            service,
            "https://charts.npmtrend.com/api",
            Duration::from_secs(60),
            CommonInterceptor::new(service),
        )
    })
}

pub fn get_image_optim_instance() -> &'static Instance<CommonInterceptor> {
    static OPTIM: OnceCell<Instance<CommonInterceptor>> = OnceCell::new();
    OPTIM.get_or_init(|| {
        let service = "image-optim";
        Instance::new(
            service,
            "http://rasp:6011",
            Duration::from_secs(60),
            CommonInterceptor::new(service),
        )
    })
}

pub use instance::Instance;
