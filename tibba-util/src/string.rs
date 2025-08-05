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

use super::timestamp;
use hex::encode;
use nanoid::nanoid;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use tibba_error::{Error, new_error};
use uuid::{NoContext, Timestamp, Uuid};

type Result<T> = std::result::Result<T, Error>;

/// Generates a UUIDv7 string
///
/// Creates a time-based UUID (version 7) using the current system time
/// Format: xxxxxxxx-xxxx-7xxx-xxxx-xxxxxxxxxxxx
///
/// # Returns
/// * String containing the formatted UUID
///
/// # Note
/// UUIDv7 provides:
/// - Timestamp-based ordering
/// - Monotonic ordering within the same timestamp
/// - Standards compliance
pub fn uuid() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ts = Timestamp::from_unix(NoContext, d.as_secs(), d.subsec_nanos());
    Uuid::new_v7(ts).to_string()
}

/// Generates a NanoID string of specified length
///
/// Creates a URL-safe, unique string using NanoID algorithm
///
/// # Arguments
/// * `size` - Length of the generated ID
///
/// # Returns
/// * String containing the NanoID
///
/// # Note
/// NanoID provides:
/// - URL-safe characters
/// - Configurable length
/// - High collision resistance
pub fn nanoid(size: usize) -> String {
    nanoid!(size)
}

/// Formats a floating-point number with specified precision
///
/// Converts float to string with fixed number of decimal places
/// Supports precision from 0 to 4 decimal places
///
/// # Arguments
/// * `value` - Floating point number to format
/// * `precision` - Number of decimal places (0-4)
///
/// # Returns
/// * String containing formatted number
///
/// # Examples
/// ```
/// assert_eq!("1.12", float_to_fixed(1.123412, 2));
/// assert_eq!("1", float_to_fixed(1.123412, 0));
/// ```
pub fn float_to_fixed(value: f64, precision: usize) -> String {
    match precision {
        0 => format!("{value:.0}"),
        1 => format!("{value:.1}"),
        2 => format!("{value:.2}"),
        3 => format!("{value:.3}"),
        _ => format!("{value:.4}"), // Default to 4 decimal places
    }
}

/// Computes the SHA-256 hash of the input data
///
/// # Arguments
/// * `data` - Input data to hash
///
/// # Returns
/// * String containing the SHA-256 hash
pub fn sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    encode(result)
}

/// Computes the SHA-256 hash of the input data and signs it with the secret key
///
/// # Arguments
/// * `value` - Input data to hash
/// * `secret` - Secret key
///
/// # Returns
/// * String containing the signed hash
pub fn sign_hash(value: &str, secret: &str) -> String {
    sha256(format!("{value}:{secret}").as_bytes())
}

/// Computes the SHA-256 hash of the input data and signs it with the secret key
///
/// # Arguments
/// * `value` - Input data to hash
/// * `secret` - Secret key
///
/// # Returns
/// * Tuple containing the timestamp and the signed hash
pub fn timestamp_hash(value: &str, secret: &str) -> (i64, String) {
    let ts = timestamp();
    let hash = sign_hash(&format!("{ts}:{value}"), secret);
    (ts, hash)
}

/// Validates the signature of the input data
///
/// # Arguments
/// * `value` - Input data to hash
/// * `hash` - Signature to validate
/// * `secret` - Secret key
///
/// # Returns
/// * Result containing the validation result
pub fn validate_sign_hash(value: &str, hash: &str, secret: &str) -> Result<()> {
    if sign_hash(value, secret) != hash {
        return Err(new_error("signature is invalid").with_category("sign_hash"));
    }
    Ok(())
}

/// Validates the signature of the input data
///
/// # Arguments
/// * `ts` - Timestamp
/// * `value` - Input data to hash
/// * `hash` - Signature to validate
/// * `secret` - Secret key
///
/// # Returns
/// * Result containing the validation result
pub fn validate_timestamp_hash(ts: i64, value: &str, hash: &str, secret: &str) -> Result<()> {
    let category = "timestamp_hash";
    if (timestamp() - ts).abs() > 5 * 60 {
        return Err(new_error("signature is expired").with_category(category));
    }
    validate_sign_hash(&format!("{ts}:{value}"), hash, secret)
}

#[cfg(test)]
mod tests {
    use super::float_to_fixed;
    use pretty_assertions::assert_eq;

    /// Tests float_to_fixed function with various precisions
    #[test]
    fn to_fixed() {
        assert_eq!("1", float_to_fixed(1.123412, 0));
        assert_eq!("1.1", float_to_fixed(1.123412, 1));
        assert_eq!("1.12", float_to_fixed(1.123412, 2));
        assert_eq!("1.123", float_to_fixed(1.123412, 3));
        assert_eq!("1.1234", float_to_fixed(1.123412, 4));
    }
}
