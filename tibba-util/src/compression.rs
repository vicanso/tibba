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
use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};

// Custom Result type using the crate's Error type
type Result<T> = std::result::Result<T, Error>;

/// An enum to represent supported compression algorithms and their parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// LZ4 algorithm.
    Lz4,
    /// Zstandard (zstd) algorithm, can specify compression level.
    Zstd(i32),
}

/// Provide a default implementation for Algorithm, for easy use.
impl Default for Algorithm {
    fn default() -> Self {
        // Default to using zstd's default compression level, as it usually has a good balance between compression ratio and speed.
        Algorithm::Zstd(zstd::DEFAULT_COMPRESSION_LEVEL)
    }
}

/// Compress data using the specified algorithm.
///
/// # Arguments
/// * `data` - Original data bytes.
/// * `algorithm` - The compression algorithm to use (`Algorithm::Lz4` or `Algorithm::Zstd(level)`).
///
/// # Returns
/// * `Result<Vec<u8>>` - Compressed data or error.
pub fn compress(data: &[u8], algorithm: Algorithm) -> Result<Vec<u8>> {
    match algorithm {
        Algorithm::Lz4 => {
            // LZ4's compression function does not return an error, so we wrap it in Ok.
            Ok(compress_prepend_size(data))
        }
        Algorithm::Zstd(level) => {
            // Optimization: use zstd::encode_all, code is more concise.
            zstd::encode_all(data, level).map_err(|e| Error::Zstd { source: e })
        }
    }
}

/// Decompress data using the specified algorithm.
///
/// # Arguments
/// * `data` - Compressed data bytes.
/// * `algorithm` - The compression algorithm used.
///
/// # Returns
/// * `Result<Vec<u8>>` - Decompressed data or error.
pub fn decompress(data: &[u8], algorithm: Algorithm) -> Result<Vec<u8>> {
    match algorithm {
        Algorithm::Lz4 => {
            decompress_size_prepended(data).map_err(|e| Error::Lz4Decompress { source: e })
        }
        // Decompression does not require compression level, so ignore the level parameter of Zstd.
        Algorithm::Zstd(_) => {
            // Optimization: use zstd::decode_all, code is more concise.
            zstd::decode_all(data).map_err(|e| Error::Zstd { source: e })
        }
    }
}
