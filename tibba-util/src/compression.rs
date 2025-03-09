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

type Result<T> = std::result::Result<T, Error>;

// lz4 decode
pub fn lz4_decode(data: &[u8]) -> Result<Vec<u8>> {
    decompress_size_prepended(data).map_err(|e| Error::Lz4Decompress { source: e })
}

// lz4 encode
pub fn lz4_encode(data: &[u8]) -> Vec<u8> {
    compress_prepend_size(data)
}

// zstd decode
pub fn zstd_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_decode(data, &mut buf).map_err(|e| Error::Zstd { source: e })?;
    Ok(buf)
}

// zstd encode
pub fn zstd_encode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_encode(data, &mut buf, zstd::DEFAULT_COMPRESSION_LEVEL)
        .map_err(|e| Error::Zstd { source: e })?;
    Ok(buf)
}
