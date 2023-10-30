use nanoid::nanoid;
use uuid::Uuid;

pub fn random_string(size: usize) -> String {
    nanoid!(size)
}

pub fn uuid() -> String {
    Uuid::new_v4().to_string()
}
