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
use std::sync::Arc;
use std::sync::RwLock;

/// Custom Result type using the crate's Error type
type Result<T> = std::result::Result<T, Error>;

/// Type alias for HMAC-SHA256 implementation
type HmacSha256 = Hmac<Sha256>;

enum KeyStore {
    Static(Vec<Vec<u8>>),
    Shared(Arc<RwLock<Vec<Vec<u8>>>>),
}

/// KeyGrip struct manages a set of cryptographic keys
/// Provides both thread-safe (RwLock) and non-thread-safe implementations
pub struct KeyGrip {
    store: KeyStore,
}

/// Helper function to create an HMAC-SHA256 signature
/// Returns the hex-encoded signature string
fn sign_with_key(data: &[u8], key: &[u8]) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|e| Error::HmacSha256 {
        message: e.to_string(),
    })?;
    mac.update(data);
    Ok(encode(mac.finalize().into_bytes()))
}

impl KeyGrip {
    /// Creates a new KeyGrip instance with non-thread-safe key storage
    /// Returns error if keys vector is empty
    pub fn new(keys: Vec<Vec<u8>>) -> Result<Self> {
        if keys.is_empty() {
            return Err(Error::KeyGripEmpty);
        }
        Ok(KeyGrip {
            store: KeyStore::Static(keys),
        })
    }

    /// Creates a new KeyGrip instance with thread-safe key storage using RwLock
    pub fn new_with_lock(keys: Vec<Vec<u8>>) -> Result<Self> {
        Ok(KeyGrip {
            store: KeyStore::Shared(Arc::new(RwLock::new(keys))),
        })
    }
    fn with_keys<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[Vec<u8>]) -> R,
    {
        match &self.store {
            KeyStore::Static(keys) => f(keys),
            KeyStore::Shared(lock_keys) => {
                // it will not fail
                if let Ok(keys) = lock_keys.read() {
                    f(&keys)
                } else {
                    f(&[])
                }
            }
        }
    }

    /// Updates the keys in the thread-safe storage
    /// No-op if using non-thread-safe storage
    pub fn update_keys(&self, new_keys: Vec<Vec<u8>>) {
        if let KeyStore::Shared(lock_keys) = &self.store
            && let Ok(mut keys) = lock_keys.write()
        {
            *keys = new_keys;
        }
    }

    /// Finds the index of the key that was used to create the given digest
    /// Returns -1 if no matching key is found
    fn index(&self, data: &[u8], digest: &str) -> Result<Option<usize>> {
        // no need to clone
        self.with_keys(|keys| {
            for (index, key) in keys.iter().enumerate() {
                // we must handle the error of sign_with_key
                match sign_with_key(data, key) {
                    Ok(signature) if signature == digest => return Ok(Some(index)),
                    // if the signature does not match, continue
                    Ok(_) => continue,
                    // if the signature process itself fails (e.g., invalid key), pass the error up
                    Err(e) => return Err(e),
                }
            }
            // if the loop ends without finding a match, return Ok(None)
            Ok(None)
        })
    }

    /// Signs the input data using the first key in the key set
    /// Returns error if no keys are available
    pub fn sign(&self, data: &[u8]) -> Result<String> {
        self.with_keys(|keys| {
            // new() has already guaranteed that keys is not empty
            sign_with_key(data, &keys[0])
        })
    }

    /// Verifies a signature (digest) against the input data
    /// Returns tuple (is_valid, is_current):
    /// - is_valid: true if signature matches any key
    /// - is_current: true if signature matches the current (first) key
    pub fn verify(&self, data: &[u8], digest: &str) -> Result<(bool, bool)> {
        match self.index(data, digest)? {
            Some(0) => Ok((true, true)),  // match the first key, valid and current
            Some(_) => Ok((true, false)), // match other keys, valid but not current
            None => Ok((false, false)),   // no match found
        }
    }
}
