use chrono::Duration;

pub fn duration_to_string(d: Duration) -> String {
    if let Ok(v) = d.to_std() {
        let value: humantime::Duration = v.into();
        return value.to_string();
    }
    "".to_string()
}
