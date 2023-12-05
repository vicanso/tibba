use chrono::{DateTime, Utc};
use std::time::Duration;

pub fn get_duration(time: &DateTime<Utc>) -> Duration {
    let mut secs = Utc::now().timestamp() - time.timestamp();
    if secs < 0 {
        secs = -secs
    }
    Duration::from_secs(secs as u64)
}

pub fn get_duration_string(time: &DateTime<Utc>) -> String {
    humantime::format_duration(get_duration(time)).to_string()
}
