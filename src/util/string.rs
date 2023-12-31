use super::timestamp;
use crate::error::HttpResult;
use crate::{config, error::HttpError};
use hex::encode;
use nanoid::nanoid;
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::{NoContext, Timestamp, Uuid};

static APP_SECRET: Lazy<String> = Lazy::new(|| config::must_new_basic_config().secret);

pub fn random_string(size: usize) -> String {
    nanoid!(size)
}

pub fn uuid() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ts = Timestamp::from_unix(NoContext, d.as_secs(), d.subsec_nanos());
    Uuid::new_v7(ts).to_string()
}

pub fn sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    encode(result)
}

pub fn sign_hash(value: &str) -> String {
    sha256(format!("{value}:{}", *APP_SECRET).as_bytes())
}
pub fn validate_sign_hash(value: &str, hash: &str) -> HttpResult<()> {
    if sign_hash(value) != hash {
        return Err(HttpError::new_with_category("数据校验不匹配", "sign_hash"));
    }
    Ok(())
}

pub fn timestamp_hash(value: &str) -> (i64, String) {
    let ts = timestamp();
    let hash = sign_hash(&format!("{ts}:{value}"));
    (ts, hash)
}

pub fn validate_timestamp_hash(ts: i64, value: &str, hash: &str) -> HttpResult<()> {
    // 超过5分钟
    let category = "timestamp_hash";
    if (timestamp() - ts).abs() > 5 * 60 {
        return Err(HttpError::new_with_category(
            "数据已过期，请刷新后重试",
            category,
        ));
    }
    validate_sign_hash(&format!("{ts}:{value}"), hash)
}
