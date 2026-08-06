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

use super::{DecompressTooLargeSnafu, Error, Lz4DecompressSnafu, ZstdSnafu};
use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};
use snafu::{ResultExt, ensure};
use std::io::Read;

// Custom Result type using the crate's Error type
type Result<T> = std::result::Result<T, Error>;

/// 默认解压上限：64 MiB。
///
/// 压缩格式天生是放大器——一个几 KB 的输入可以声称/展开出数 GB。`decompress`
/// 的输入来自 Redis 等外部存储，一旦数据损坏或被篡改，无上限解压就是一次
/// OOM。64 MiB 对缓存条目远远够用；确需更大请显式用
/// [`decompress_with_limit`]。
pub const DEFAULT_DECOMPRESS_LIMIT: usize = 64 * 1024 * 1024;

/// lz4 帧头部存放未压缩长度的字节数（小端 u32）。
const LZ4_SIZE_PREFIX_LEN: usize = 4;

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

/// 解压数据，输出上限为 [`DEFAULT_DECOMPRESS_LIMIT`]。
///
/// 需要自定义上限时用 [`decompress_with_limit`]。
pub fn decompress(data: &[u8], algorithm: Algorithm) -> Result<Vec<u8>> {
    decompress_with_limit(data, algorithm, DEFAULT_DECOMPRESS_LIMIT)
}

/// 解压数据，输出超过 `limit` 字节即报错。
///
/// # 为什么必须有上限
/// - **lz4**：`compress_prepend_size` 把未压缩长度写在头 4 字节，而
///   `decompress_size_prepended` 会**直接按这个值预分配**。一个 8 字节的
///   损坏 / 恶意 blob 只要把长度字段填成 `0xFFFFFFFF`，就能让进程立刻申请 4 GiB。
/// - **zstd**：格式本身支持极高压缩比，几 KB 输入可展开出数 GB。
///
/// 两条路径都在**分配之前**或**分配过程中**卡住，而不是解压完再检查大小。
pub fn decompress_with_limit(data: &[u8], algorithm: Algorithm, limit: usize) -> Result<Vec<u8>> {
    match algorithm {
        Algorithm::Lz4 => {
            // 先读头部声明的长度做校验，避免 decompress_size_prepended 按它预分配
            if let Some(prefix) = data.get(..LZ4_SIZE_PREFIX_LEN) {
                let mut buf = [0u8; LZ4_SIZE_PREFIX_LEN];
                buf.copy_from_slice(prefix);
                let declared = u32::from_le_bytes(buf) as usize;
                ensure!(
                    declared <= limit,
                    DecompressTooLargeSnafu {
                        size: declared,
                        limit
                    }
                );
            }
            // 长度不足 4 字节的输入交给库报格式错误，信息比自造的更准确
            decompress_size_prepended(data).context(Lz4DecompressSnafu)
        }
        // 解压不需要压缩级别，忽略 Zstd 的 level 参数
        Algorithm::Zstd(_) => {
            let decoder = zstd::Decoder::new(data).context(ZstdSnafu)?;
            let mut out = Vec::new();
            // 多读 1 字节：读满 limit+1 说明真实输出已超限，否则无法区分
            // 「恰好等于上限」与「被截断」
            decoder
                .take(limit as u64 + 1)
                .read_to_end(&mut out)
                .context(ZstdSnafu)?;
            ensure!(
                out.len() <= limit,
                DecompressTooLargeSnafu {
                    size: out.len(),
                    limit
                }
            );
            Ok(out)
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

    /// lz4 的长度前缀被篡改成天文数字时，必须在**预分配之前**拦下。
    ///
    /// 这条是本文件最关键的守卫：`decompress_size_prepended` 会直接按前缀
    /// 预分配，去掉上限检查后本例会让进程尝试申请 4 GiB。
    #[test]
    fn lz4_forged_size_prefix_is_rejected_before_allocating() {
        let mut forged = compress(b"tiny payload", Algorithm::Lz4).expect("compress");
        // 把头 4 字节（小端 u32 未压缩长度）改成 0xFFFFFFFF ≈ 4 GiB
        forged[..4].copy_from_slice(&u32::MAX.to_le_bytes());

        let err = decompress(&forged, Algorithm::Lz4).expect_err("伪造的超大长度必须被拒绝");
        assert!(
            err.to_string().contains("exceeds limit"),
            "应报超限而非其它错误，实际: {err}"
        );
    }

    #[test]
    fn limit_is_enforced_for_both_algorithms() {
        let payload = vec![b'a'; 4096];
        for algo in [Algorithm::Lz4, Algorithm::Zstd(3)] {
            let compressed = compress(&payload, algo).expect("compress");

            // 上限小于真实输出 → 报错
            let err = decompress_with_limit(&compressed, algo, 1024)
                .expect_err("{algo:?} 超限时必须报错");
            assert!(err.to_string().contains("exceeds limit"), "{err}");

            // 上限恰好等于真实输出 → 放行（边界不能误杀）
            let out = decompress_with_limit(&compressed, algo, payload.len())
                .expect("恰好等于上限应当放行");
            assert_eq!(out, payload);
        }
    }

    /// zstd 高压缩比炸弹：输入很小，输出远超上限，必须被拦住。
    #[test]
    fn zstd_bomb_is_capped() {
        // 16 MiB 全零，zstd 压完只有几十字节
        let bomb = compress(&vec![0u8; 16 * 1024 * 1024], Algorithm::Zstd(3)).expect("compress");
        assert!(bomb.len() < 4096, "构造前提：压缩后应当很小");

        let err = decompress_with_limit(&bomb, Algorithm::Zstd(3), 1024)
            .expect_err("高压缩比炸弹必须被上限拦下");
        assert!(err.to_string().contains("exceeds limit"), "{err}");
    }
}
