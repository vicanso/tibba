use snafu::{ResultExt, Snafu};
use snap::{read::FrameDecoder, write::FrameEncoder};
use std::io::{Read, Write};

use crate::error::HttpError;

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("Io {category}: {source}"))]
    Io {
        category: String,
        source: std::io::Error,
    },
    #[snafu(display("Error {category}: {source}"))]
    Whatever {
        category: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
impl From<Error> for HttpError {
    fn from(err: Error) -> Self {
        match err {
            Error::Io { category, source } => {
                HttpError::new_with_category(&source.to_string(), &category)
            }
            Error::Whatever { category, source } => {
                HttpError::new_with_category(&source.to_string(), &category)
            }
        }
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

// snappy解压
pub fn snappy_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    FrameDecoder::new(data)
        .read_to_end(&mut buf)
        .context(IoSnafu {
            category: "snappy_decode",
        })?;

    Ok(buf)
}

// snappy压缩
pub fn snappy_encode(data: &[u8]) -> Result<Vec<u8>> {
    let mut writer = FrameEncoder::new(vec![]);
    writer.write_all(data).context(IoSnafu {
        category: "snappy_encode",
    })?;
    let data = writer
        .into_inner()
        .map_err(|e| Box::new(e) as _)
        .context(WhateverSnafu {
            category: "snappy_encode",
        })?;
    Ok(data)
}

// zstd解压
pub fn zstd_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_decode(data, &mut buf).context(IoSnafu {
        category: "zstd_decode",
    })?;
    Ok(buf)
}

// zstd解压
pub fn zstd_encode(data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_encode(data, &mut buf, zstd::DEFAULT_COMPRESSION_LEVEL).context(
        IoSnafu {
            category: "zstd_encode",
        },
    )?;
    Ok(buf)
}
