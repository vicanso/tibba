use crate::error::{HttpError, HttpResult};
use hex::encode;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub struct KeyGrip {
    keys: Vec<Vec<u8>>,
}

fn sign(data: &[u8], key: &[u8]) -> HttpResult<String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|err| HttpError::new_with_category(&err.to_string(), "keygrip"))?;
    mac.update(data);
    let result = mac.finalize();
    Ok(encode(result.into_bytes()))
}

impl KeyGrip {
    pub fn new(keys: Vec<Vec<u8>>) -> Self {
        if keys.is_empty() {
            panic!("keys is empty")
        }
        KeyGrip { keys }
    }
    fn index(&self, data: &[u8], digest: &str) -> HttpResult<i64> {
        for (index, key) in self.keys.iter().enumerate() {
            if sign(data, key)?.eq(digest) {
                return Ok(index as i64);
            }
        }
        Ok(-1)
    }
    pub fn sign(&self, data: &[u8]) -> HttpResult<String> {
        sign(data, &self.keys[0])
    }
    pub fn verify(&self, data: &[u8], digest: &str) -> HttpResult<(bool, bool)> {
        let value = self.index(data, digest)?;
        Ok((value >= 0, value == 0))
    }
}
