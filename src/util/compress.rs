use snap::{read::FrameDecoder, write::FrameEncoder};
use std::io::{Read, Write};

use crate::error::{HTTPError, HTTPResult};

// snappy解压
pub fn snappy_decode(data: &[u8]) -> HTTPResult<Vec<u8>> {
    let mut buf = vec![];
    FrameDecoder::new(data).read_to_end(&mut buf)?;

    Ok(buf)
}

// snappy压缩
pub fn snappy_encode(data: &[u8]) -> HTTPResult<Vec<u8>> {
    let mut writer = FrameEncoder::new(vec![]);
    writer.write_all(data)?;
    let data = writer
        .into_inner()
        .map_err(|err| HTTPError::new(err.to_string().as_str()))?;
    Ok(data)
}

// zstd解压
pub fn zstd_decode(data: &[u8]) -> HTTPResult<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_decode(data, &mut buf)
        .map_err(|err| HTTPError::new(err.to_string().as_str()))?;
    Ok(buf)
}

// zstd解压
pub fn zstd_encode(data: &[u8]) -> HTTPResult<Vec<u8>> {
    let mut buf = vec![];
    zstd::stream::copy_encode(data, &mut buf, zstd::DEFAULT_COMPRESSION_LEVEL)
        .map_err(|err| HTTPError::new(err.to_string().as_str()))?;
    Ok(buf)
}
