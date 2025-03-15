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

// Import necessary dependencies for cryptographic operations and error handling
use super::Error;
use hex::encode;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::RwLock;

// Type alias for HMAC-SHA256 implementation
type HmacSha256 = Hmac<Sha256>;

// Custom Result type using the crate's Error type
type Result<T> = std::result::Result<T, Error>;

// KeyGrip struct manages a set of cryptographic keys
// Provides both thread-safe (RwLock) and non-thread-safe implementations
pub struct KeyGrip {
    // Non-thread-safe keys storage
    keys: Option<Vec<Vec<u8>>>,
    // Thread-safe keys storage using RwLock
    lock_keys: Option<RwLock<Vec<Vec<u8>>>>,
}

// Helper function to create an HMAC-SHA256 signature
// Returns the hex-encoded signature string
fn sign(data: &[u8], key: &[u8]) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|e| Error::HmacSha256 {
        message: e.to_string(),
    })?;
    mac.update(data);
    let result = mac.finalize();
    Ok(encode(result.into_bytes()))
}

impl KeyGrip {
    // Creates a new KeyGrip instance with non-thread-safe key storage
    // Returns error if keys vector is empty
    pub fn new(keys: Vec<Vec<u8>>) -> Result<Self> {
        if keys.is_empty() {
            return Err(Error::KeyGripEmpty);
        }
        Ok(KeyGrip {
            keys: Some(keys),
            lock_keys: None,
        })
    }

    // Creates a new KeyGrip instance with thread-safe key storage using RwLock
    pub fn new_with_lock(keys: Vec<Vec<u8>>) -> Result<Self> {
        Ok(KeyGrip {
            keys: None,
            lock_keys: Some(RwLock::new(keys)),
        })
    }

    // Updates the keys in the thread-safe storage
    // No-op if using non-thread-safe storage
    pub fn update_keys(&self, new_keys: Vec<Vec<u8>>) {
        if let Some(lock_keys) = &self.lock_keys {
            if let Ok(mut keys) = lock_keys.write() {
                *keys = new_keys;
            }
        }
    }

    // Internal method to retrieve current keys
    // Handles both thread-safe and non-thread-safe implementations
    fn get_keys(&self) -> Vec<Vec<u8>> {
        if let Some(keys) = &self.lock_keys {
            if let Ok(keys) = keys.read() {
                return keys.clone();
            }
        }
        if let Some(keys) = &self.keys {
            return keys.clone();
        }
        vec![]
    }

    // Finds the index of the key that was used to create the given digest
    // Returns -1 if no matching key is found
    fn index(&self, data: &[u8], digest: &str) -> Result<i64> {
        for (index, key) in self.get_keys().iter().enumerate() {
            if sign(data, key)?.eq(digest) {
                return Ok(index as i64);
            }
        }
        Ok(-1)
    }

    // Signs the input data using the first key in the key set
    // Returns error if no keys are available
    pub fn sign(&self, data: &[u8]) -> Result<String> {
        let keys = self.get_keys();
        if keys.is_empty() {
            return Err(Error::KeyGripEmpty);
        }
        sign(data, &keys[0])
    }

    // Verifies a signature (digest) against the input data
    // Returns tuple (is_valid, is_current):
    // - is_valid: true if signature matches any key
    // - is_current: true if signature matches the current (first) key
    pub fn verify(&self, data: &[u8], digest: &str) -> Result<(bool, bool)> {
        let value = self.index(data, digest)?;
        Ok((value >= 0, value == 0))
    }
}
