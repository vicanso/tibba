use chrono::{DateTime, Utc};
use once_cell::sync::OnceCell;
use std::sync::atomic::{AtomicI32, AtomicI8, Ordering};

use crate::config::must_new_basic_config;

#[derive(Debug)]
pub struct AppState {
    pub processing_limit: i32,
    status: AtomicI8,
    processing: AtomicI32,
    started_at: DateTime<Utc>,
}

const APP_STATUS_STOP: i8 = 0;
const APP_STATUS_RUNNING: i8 = 1;

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
    pub fn is_running(&self) -> bool {
        let value = self.status.load(Ordering::Relaxed);
        value == APP_STATUS_RUNNING
    }
    pub fn run(&self) {
        self.status.store(APP_STATUS_RUNNING, Ordering::Relaxed)
    }
    pub fn stop(&self) {
        self.status.store(APP_STATUS_STOP, Ordering::Relaxed)
    }
    pub fn get_started_at(&self) -> DateTime<Utc> {
        self.started_at
    }
}

pub fn get_app_state() -> &'static AppState {
    static APP_STATE: OnceCell<AppState> = OnceCell::new();
    APP_STATE.get_or_init(|| {
        // 在main时已调用，因此不会unwrap
        let basic_config = must_new_basic_config();
        AppState {
            processing_limit: basic_config.processing_limit,
            started_at: Utc::now(),
            status: AtomicI8::new(0),
            processing: AtomicI32::new(0),
        }
    })
}
