pub fn clone_value_from_task_local<T>(value: &T) -> T
where
    T: Clone,
{
    value.clone()
}
