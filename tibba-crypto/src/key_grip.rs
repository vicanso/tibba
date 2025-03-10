// Copyright 2025 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::Error;
use hex::encode;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

type Result<T> = std::result::Result<T, Error>;

pub struct KeyGrip {
    keys: Vec<Vec<u8>>,
}

fn sign(data: &[u8], key: &[u8]) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|e| Error::HmacSha256 {
        message: e.to_string(),
    })?;
    mac.update(data);
    let result = mac.finalize();
    Ok(encode(result.into_bytes()))
}

impl KeyGrip {
    pub fn new(keys: Vec<Vec<u8>>) -> Result<Self> {
        if keys.is_empty() {
            return Err(Error::KeyGripEmpty);
        }
        Ok(KeyGrip { keys })
    }
    fn index(&self, data: &[u8], digest: &str) -> Result<i64> {
        for (index, key) in self.keys.iter().enumerate() {
            if sign(data, key)?.eq(digest) {
                return Ok(index as i64);
            }
        }
        Ok(-1)
    }
    pub fn sign(&self, data: &[u8]) -> Result<String> {
        sign(data, &self.keys[0])
    }
    pub fn verify(&self, data: &[u8], digest: &str) -> Result<(bool, bool)> {
        let value = self.index(data, digest)?;
        Ok((value >= 0, value == 0))
    }
}
