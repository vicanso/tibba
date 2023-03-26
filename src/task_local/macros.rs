// task local log
#[macro_export]
macro_rules! tl_info {
    ($($arg:tt)*) => (
        let trace_id = TRACE_ID.with(clone_value_from_task_local);
        let device_id = DEVICE_ID.with(clone_value_from_task_local);
        let account = ACCOUNT.with(clone_value_from_task_local);
        info!(
            deviceId = device_id,
            traceId = trace_id,
            account,
            $($arg)*
        )
    );
}

#[macro_export]
macro_rules! tl_error {
    ($($arg:tt)*) => (
        let trace_id = TRACE_ID.with(clone_value_from_task_local);
        let device_id = DEVICE_ID.with(clone_value_from_task_local);
        let account = ACCOUNT.with(clone_value_from_task_local);
        error!(
            deviceId = device_id,
            traceId = trace_id,
            account,
            $($arg)*
        )
    );
}

#[macro_export]
macro_rules! tl_warn {
    ($($arg:tt)*) => (
        let trace_id = TRACE_ID.with(clone_value_from_task_local);
        let device_id = DEVICE_ID.with(clone_value_from_task_local);
        let account = ACCOUNT.with(clone_value_from_task_local);
        warn!(
            deviceId = device_id,
            traceId = trace_id,
            account,
            $($arg)*
        )
    );
}
