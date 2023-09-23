use chrono::Duration;

pub fn duration_to_string(mut d: Duration) -> String {
    if d < Duration::zero() {
        d = -d;
    }
    // 已保证一定>=0，因此不会出错
    let value: humantime::Duration = d.to_std().unwrap().into();
    value.to_string()
}
