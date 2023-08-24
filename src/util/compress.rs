use crate::error::HttpError;
use lz4_flex::block::{compress_prepend_size, decompress_size_prepended, DecompressError};
use snafu::{ResultExt, Snafu};

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("Io {category}: {source}"))]
    Io {
        category: String,
        source: std::io::Error,
    },
    #[snafu(display("{source}"))]
    Lz4Decompress { source: DecompressError },
}
impl From<Error> for HttpError {
    fn from(err: Error) -> Self {
        match err {
            Error::Io { category, source } => {
                HttpError::new_with_category(&source.to_string(), &category)
            }
            Error::Lz4Decompress { source } => {
                HttpError::new_with_category(&source.to_string(), "lz4")
            }
        }
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

// zstd解压
pub fn zstd_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_decode(data, &mut buf).context(IoSnafu {
        category: "zstd_decode",
    })?;
    Ok(buf)
}

// zstd压缩
pub fn zstd_encode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_encode(data, &mut buf, zstd::DEFAULT_COMPRESSION_LEVEL).context(
        IoSnafu {
            category: "zstd_encode",
        },
    )?;
    Ok(buf)
}

// lz4解压
pub fn lz4_decode(data: &[u8]) -> Result<Vec<u8>> {
    decompress_size_prepended(data).context(Lz4DecompressSnafu {})
}

// lz4压缩
pub fn lz4_encode(data: &[u8]) -> Vec<u8> {
    compress_prepend_size(data)
}
