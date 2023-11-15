use chrono::{offset, Local, NaiveDateTime};

pub fn now() -> String {
    Local::now().to_string()
}

pub fn timestamp() -> i64 {
    Local::now().timestamp()
}

pub fn from_timestamp(secs: i64, nsecs: u32) -> String {
    if let Some(value) = NaiveDateTime::from_timestamp_opt(secs, nsecs) {
        value.and_utc().with_timezone(&offset::Local).to_string()
    } else {
        "".to_string()
    }
}
