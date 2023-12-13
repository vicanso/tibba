use hex::encode;
use nanoid::nanoid;
use sha2::{Digest, Sha256};
use uuid::{Uuid, Timestamp, NoContext};
use std::time::{SystemTime, UNIX_EPOCH};


pub fn random_string(size: usize) -> String {
    nanoid!(size)
}

pub fn uuid() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let ts = Timestamp::from_unix(NoContext, d.as_secs(), d.subsec_nanos());
    Uuid::new_v7(ts).to_string()
}

pub fn sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    encode(result)
}
