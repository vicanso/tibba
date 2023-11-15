use hex::encode;
use nanoid::nanoid;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn random_string(size: usize) -> String {
    nanoid!(size)
}

pub fn uuid() -> String {
    Uuid::new_v4().to_string()
}

pub fn sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    encode(&result)
}
