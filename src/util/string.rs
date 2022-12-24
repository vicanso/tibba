use nanoid::nanoid;

pub fn random_string(size: usize) -> String {
    nanoid!(size)
}
