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

/// Decompresses LZ4 compressed data
///
/// Uses size-prepended format where the original size is stored at the start
/// of the compressed data
///
/// # Arguments
/// * `data` - Compressed data bytes
///
/// # Returns
/// * `Result<Vec<u8>>` - Decompressed data or error
pub fn lz4_decode(data: &[u8]) -> Result<Vec<u8>> {
    decompress_size_prepended(data).map_err(|e| Error::Lz4Decompress { source: e })
}

/// Compresses data using LZ4 algorithm
///
/// Prepends the original size to the compressed data to allow for
/// proper decompression later
///
/// # Arguments
/// * `data` - Raw data bytes to compress
///
/// # Returns
/// * `Vec<u8>` - Compressed data with prepended size
pub fn lz4_encode(data: &[u8]) -> Vec<u8> {
    compress_prepend_size(data)
}

/// Decompresses Zstandard (zstd) compressed data
///
/// Uses streaming decompression for better memory efficiency
///
/// # Arguments
/// * `data` - Compressed data bytes
///
/// # Returns
/// * `Result<Vec<u8>>` - Decompressed data or error
pub fn zstd_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_decode(data, &mut buf).map_err(|e| Error::Zstd { source: e })?;
    Ok(buf)
}

/// Compresses data using Zstandard (zstd) algorithm
///
/// Uses streaming compression with default compression level
///
/// # Arguments
/// * `data` - Raw data bytes to compress
///
/// # Returns
/// * `Result<Vec<u8>>` - Compressed data or error
pub fn zstd_encode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_encode(data, &mut buf, zstd::DEFAULT_COMPRESSION_LEVEL)
        .map_err(|e| Error::Zstd { source: e })?;
    Ok(buf)
}
