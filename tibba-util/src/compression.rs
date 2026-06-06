// Copyright 2026 Tree xie.
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

use super::{Error, Lz4DecompressSnafu, ZstdSnafu};
use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};
use snafu::ResultExt;

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
            zstd::encode_all(data, level).context(ZstdSnafu)
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
        Algorithm::Lz4 => decompress_size_prepended(data).context(Lz4DecompressSnafu),
        // Decompression does not require compression level, so ignore the level parameter of Zstd.
        Algorithm::Zstd(_) => {
            // Optimization: use zstd::decode_all, code is more concise.
            zstd::decode_all(data).context(ZstdSnafu)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// 可重复运行的压缩 / 解压回路：原始字节 → 压缩 → 解压 → 与原始一致
    fn round_trip(algo: Algorithm, payload: &[u8]) {
        let compressed = compress(payload, algo).expect("compress should succeed");
        let decompressed = decompress(&compressed, algo).expect("decompress should succeed");
        assert_eq!(decompressed, payload, "round-trip 必须保留所有字节");
    }

    #[test]
    fn lz4_round_trip() {
        round_trip(Algorithm::Lz4, b"hello world");
        round_trip(Algorithm::Lz4, &vec![0xAB; 1024]); // 压缩友好的重复数据
        round_trip(Algorithm::Lz4, &[]); // 空输入
    }

    #[test]
    fn zstd_round_trip() {
        let algo = Algorithm::Zstd(zstd::DEFAULT_COMPRESSION_LEVEL);
        round_trip(algo, b"hello world");
        round_trip(algo, &vec![0xAB; 1024]);
        round_trip(algo, &[]);
    }

    #[test]
    fn decompress_garbage_returns_error() {
        // 截断 / 非法的压缩数据必须报错，不能产生空字节当成功
        let garbage = b"this is definitely not compressed data";
        assert!(decompress(garbage, Algorithm::Lz4).is_err());
        assert!(decompress(garbage, Algorithm::Zstd(0)).is_err());
    }
}
