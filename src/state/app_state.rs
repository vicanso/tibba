use once_cell::sync::OnceCell;
use std::sync::atomic::{AtomicI32, Ordering};

#[derive(Debug)]
pub struct AppState {
    processing: AtomicI32,
}

impl AppState {
    pub fn increase_processing(&self) -> i32 {
        self.processing.fetch_add(1, Ordering::Relaxed)
    }
    pub fn decrease_processing(&self) -> i32 {
        self.processing.fetch_add(-1, Ordering::Relaxed)
    }
    pub fn get_processing(&self) -> i32 {
        self.processing.load(Ordering::Relaxed)
    }
}

static APP_STATE: OnceCell<AppState> = OnceCell::new();

pub fn get_app_state() -> &'static AppState {
    APP_STATE.get_or_init(|| AppState {
        processing: AtomicI32::new(0),
    })
}
