use snafu::{ResultExt, Whatever};
use snap::{read::FrameDecoder, write::FrameEncoder};
use std::io::{Read, Write};

// snappy解压
pub fn snappy_decode(data: &[u8]) -> Result<Vec<u8>, Whatever> {
    let mut buf = vec![];
    FrameDecoder::new(data)
        .read_to_end(&mut buf)
        .with_whatever_context(|err| format!("Read all fail {err}"))?;

    Ok(buf)
}

// snappy压缩
pub fn snappy_encode(data: &[u8]) -> Result<Vec<u8>, Whatever> {
    let mut writer = FrameEncoder::new(vec![]);
    writer
        .write_all(data)
        .with_whatever_context(|err| format!("Write all fail {err}"))?;
    let data = writer
        .into_inner()
        .with_whatever_context(|err| format!("To inner fail {err}"))?;
    Ok(data)
}

// zstd解压
pub fn zstd_decode(data: &[u8]) -> Result<Vec<u8>, Whatever> {
    let mut buf = vec![];
    zstd::stream::copy_decode(data, &mut buf)
        .with_whatever_context(|err| format!("Zstd decode fail {err}"))?;
    Ok(buf)
}

// zstd解压
pub fn zstd_encode(data: &[u8]) -> Result<Vec<u8>, Whatever> {
    let mut buf = vec![];
    zstd::stream::copy_encode(data, &mut buf, zstd::DEFAULT_COMPRESSION_LEVEL)
        .with_whatever_context(|err| format!("Zstd encode fail {err}"))?;
    Ok(buf)
}
